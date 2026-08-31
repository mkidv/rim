// SPDX-License-Identifier: MIT

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

        // Clear sectors 0..15 (System Area)
        let zeros = [0u8; ISO_SECTOR_SIZE];
        for lba in 0..16 {
            self.io.write_at(lba * ISO_SECTOR_SIZE as u64, &zeros)?;
        }

        // Sector 16: Primary Volume Descriptor (PVD)
        let mut pvd = [0u8; ISO_SECTOR_SIZE];
        pvd[0] = VD_PRIMARY;
        pvd[1..6].copy_from_slice(ISO_STANDARD_ID);
        pvd[6] = 1; // Version

        // System Identifier (8..40, 32 bytes)
        let sys_id = b"LINUX                           ";
        pvd[8..40].copy_from_slice(sys_id);

        // Volume Identifier (40..72, 32 bytes)
        let mut vol_id = [b' '; 32];
        let label_bytes = self.meta.volume_id.as_bytes();
        let len = label_bytes.len().min(32);
        vol_id[..len].copy_from_slice(&label_bytes[..len]);
        pvd[40..72].copy_from_slice(&vol_id);

        let total_sectors = 25u32;
        put_both_u32(&mut pvd[80..88], total_sectors); // Volume Space Size
        put_both_u16(&mut pvd[120..124], 1); // Volume Set Size
        put_both_u16(&mut pvd[124..128], 1); // Volume Sequence Number
        put_both_u16(&mut pvd[128..132], ISO_SECTOR_SIZE as u16); // Logical Block Size
        put_both_u32(&mut pvd[132..140], 10); // Path Table Size (10 bytes)

        // Type L and Type M Path Table LBAs
        pvd[140..144].copy_from_slice(&18u32.to_le_bytes()); // Type L Path Table at sector 18
        pvd[148..152].copy_from_slice(&19u32.to_be_bytes()); // Type M Path Table at sector 19

        // Root Directory Record in PVD (156..190, 34 bytes)
        let root_lba = 20u32;
        let root_size = ISO_SECTOR_SIZE as u32;
        pvd[156] = 34; // Record length
        pvd[157] = 0; // Extended attr length
        put_both_u32(&mut pvd[158..166], root_lba);
        put_both_u32(&mut pvd[166..174], root_size);
        pvd[174..181].copy_from_slice(&now_bin);
        pvd[181] = DIR_FLAG_DIRECTORY;
        put_both_u16(&mut pvd[184..188], 1);
        pvd[188] = 1; // File ID length
        pvd[189] = 0; // Root directory ID (\0)

        // Dates
        pvd[813..830].copy_from_slice(&now_text); // Creation date
        pvd[830..847].copy_from_slice(&now_text); // Mod date
        pvd[881] = 1; // File structure version

        self.io.write_at(16 * ISO_SECTOR_SIZE as u64, &pvd)?;

        // Sector 17: Terminator
        let mut term = [0u8; ISO_SECTOR_SIZE];
        term[0] = VD_TERMINATOR;
        term[1..6].copy_from_slice(ISO_STANDARD_ID);
        term[6] = 1;
        self.io.write_at(17 * ISO_SECTOR_SIZE as u64, &term)?;

        // Sector 18: Type L Path Table (LE)
        let mut pt_l = [0u8; ISO_SECTOR_SIZE];
        pt_l[0] = 1; // Name len
        pt_l[1] = 0; // Ext attr len
        pt_l[2..6].copy_from_slice(&root_lba.to_le_bytes());
        pt_l[6..8].copy_from_slice(&1u16.to_le_bytes()); // Parent directory number = 1
        pt_l[8] = 0; // Name (\0)
        pt_l[9] = 0; // Pad
        self.io.write_at(18 * ISO_SECTOR_SIZE as u64, &pt_l)?;

        // Sector 19: Type M Path Table (BE)
        let mut pt_m = [0u8; ISO_SECTOR_SIZE];
        pt_m[0] = 1;
        pt_m[1] = 0;
        pt_m[2..6].copy_from_slice(&root_lba.to_be_bytes());
        pt_m[6..8].copy_from_slice(&1u16.to_be_bytes());
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
