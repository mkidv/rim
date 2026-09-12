// SPDX-License-Identifier: MIT

//! ISO 9660 volume formatter and descriptor table generator.

use crate::records::{IsoPathTableHeaderBe, IsoPathTableHeaderLe};
use rimio::RimWriteStructExt;
use zerocopy::IntoBytes;

#[cfg(feature = "alloc")]
extern crate alloc;

use crate::meta::IsoMeta;
use crate::types::*;
use rimfs_core::errors::FsFormatterResult;
use rimfs_core::formatter::FsFormatter;
use rimio::RimIO;

/// Initializes and writes a minimal valid empty ISO 9660 image structure.
pub struct IsoFormatter<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a IsoMeta,
}

impl<'a, IO: RimIO + ?Sized> IsoFormatter<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a IsoMeta) -> Self {
        Self { io, meta }
    }
}

impl<'a, IO: RimIO + ?Sized> FsFormatter for IsoFormatter<'a, IO> {
    fn format(&mut self, _is_quick: bool) -> FsFormatterResult {
        let now = rimfs_core::utils::time_utils::now_utc();
        let now_text = format_iso_text_datetime(now);
        let now_bin = format_iso_binary_datetime(now);

        self.io.zero_at(0, 16 * ISO_SECTOR_SIZE as u64)?;

        // Sector 16: Primary Volume Descriptor (PVD)
        let mut pvd = IsoVolumeDescriptor {
            kind: VD_PRIMARY,
            ..Default::default()
        };
        pvd.standard_id.copy_from_slice(ISO_STANDARD_ID);
        pvd.version = 1; // Version

        // System Identifier (8..40, 32 bytes)
        let sys_id = b"LINUX                           ";
        pvd.system_id.copy_from_slice(sys_id);

        // Volume Identifier (40..72, 32 bytes)
        let mut vol_id = [b' '; 32];
        let label_bytes = self.meta.volume_id.as_bytes();
        let len = label_bytes.len().min(32);
        vol_id[..len].copy_from_slice(&label_bytes[..len]);
        pvd.volume_id.copy_from_slice(&vol_id);

        let total_sectors = 25u32;
        pvd.volume_space_size = (total_sectors).into(); // Volume Space Size
        pvd.volume_set_size = (1).into(); // Volume Set Size
        pvd.volume_sequence = (1).into(); // Volume Sequence Number
        pvd.logical_block_size = (ISO_SECTOR_SIZE as u16).into(); // Logical Block Size
        pvd.path_table_size = (10).into(); // Path Table Size (10 bytes)

        // Type L and Type M Path Table LBAs
        pvd.path_table_l = (18u32).into(); // Type L Path Table at sector 18
        pvd.path_table_m = (19u32).into(); // Type M Path Table at sector 19

        // Root Directory Record in PVD (156..190, 34 bytes)
        let root_lba = 20u32;
        let root_size = ISO_SECTOR_SIZE as u32;
        pvd.root.header.record_len = 34; // Record length
        pvd.root.header.extended_attr_len = 0; // Extended attr length
        pvd.root.header.extent_lba = (root_lba).into();
        pvd.root.header.data_length = (root_size).into();
        pvd.root.header.recorded.copy_from_slice(&now_bin);
        pvd.root.header.flags = DIR_FLAG_DIRECTORY;
        pvd.root.header.volume_sequence = (1).into();
        pvd.root.header.name_len = 1; // File ID length
        pvd.root.identifier = 0; // Root directory ID (\0)

        // Dates
        pvd.created.copy_from_slice(&now_text); // Creation date
        pvd.modified.copy_from_slice(&now_text); // Mod date
        pvd.file_structure_version = 1; // File structure version

        self.io.write_struct(16 * ISO_SECTOR_SIZE as u64, &pvd)?;

        // Sector 17: Terminator
        let mut term = IsoTerminator {
            kind: VD_TERMINATOR,
            ..Default::default()
        };
        term.standard_id.copy_from_slice(ISO_STANDARD_ID);
        term.version = 1;
        self.io.write_struct(17 * ISO_SECTOR_SIZE as u64, &term)?;

        // Sector 18: Type L Path Table (LE)
        let mut pt_l = [0u8; ISO_SECTOR_SIZE];
        let header = IsoPathTableHeaderLe {
            identifier_len: 1,
            extended_attribute_len: 0,
            extent_lba: root_lba.into(),
            parent_number: 1.into(),
        };
        pt_l[..8].copy_from_slice(header.as_bytes());
        pt_l[8] = 0; // Name (\0)
        pt_l[9] = 0; // Pad
        self.io.write_at(18 * ISO_SECTOR_SIZE as u64, &pt_l)?;

        // Sector 19: Type M Path Table (BE)
        let mut pt_m = [0u8; ISO_SECTOR_SIZE];
        let header = IsoPathTableHeaderBe {
            identifier_len: 1,
            extended_attribute_len: 0,
            extent_lba: root_lba.into(),
            parent_number: 1.into(),
        };
        pt_m[..8].copy_from_slice(header.as_bytes());
        pt_m[8] = 0;
        pt_m[9] = 0;
        self.io.write_at(19 * ISO_SECTOR_SIZE as u64, &pt_m)?;

        // Sector 20: Empty Root Directory (. and .. entries)
        let mut root_dir = [0u8; ISO_SECTOR_SIZE];
        // "." entry (offset 0, len 34)
        root_dir[0] = 34;
        put_both_u32(&mut root_dir[2..10], root_lba);
        put_both_u32(&mut root_dir[10..18], root_size);
        root_dir[18..25].copy_from_slice(&now_bin);
        root_dir[25] = DIR_FLAG_DIRECTORY;
        put_both_u16(&mut root_dir[28..32], 1);
        root_dir[32] = 1;
        root_dir[33] = 0; // \0 for .

        // ".." entry (offset 34, len 34)
        root_dir[34] = 34;
        put_both_u32(&mut root_dir[36..44], root_lba);
        put_both_u32(&mut root_dir[44..52], root_size);
        root_dir[52..59].copy_from_slice(&now_bin);
        root_dir[59] = DIR_FLAG_DIRECTORY;
        put_both_u16(&mut root_dir[62..66], 1);
        root_dir[66] = 1;
        root_dir[67] = 1; // \1 for ..

        self.io.write_at(20 * ISO_SECTOR_SIZE as u64, &root_dir)?;
        self.io.flush()?;
        Ok(())
    }
}
