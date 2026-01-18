// SPDX-License-Identifier: MIT

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use time::OffsetDateTime;

use crate::layout::Layout;
use crate::out::img;
use crate::out::target::DryRunMode;

use zerocopy::byteorder::{BigEndian, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

#[repr(C)]
#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Clone, Copy)]
pub struct VhdFooter {
    pub cookie: [u8; 8],                 // "conectix"
    pub features: U32<BigEndian>,        // 0x0000_0002
    pub file_format_ver: U32<BigEndian>, // 0x0001_0000
    pub data_offset: U64<BigEndian>,     // 0xFFFF_FFFF_FFFF_FFFF
    pub timestamp: U32<BigEndian>,       // seconds since 2000-01-01
    pub creator_app: [u8; 4],            // e.g. b"rim\0"
    pub creator_ver: U32<BigEndian>,     // e.g. 0x000A_0000
    pub creator_os: [u8; 4],             // e.g. b"Wi2k"
    pub orig_size: U64<BigEndian>,       // disk bytes (padded to 512 before footer)
    pub curr_size: U64<BigEndian>,       // same as orig_size for fixed
    pub geometry_cyls: [u8; 2],          // CHS (cylinders BE)
    pub geometry_heads: u8,              // heads
    pub geometry_sects: u8,              // sectors/track
    pub disk_type: U32<BigEndian>,       // 2 = fixed
    pub checksum: U32<BigEndian>,        // ones' complement of sum of all bytes w/ checksum=0
    pub unique_id: [u8; 16],             // UUID
    pub saved_state: u8,                 // 0
    pub reserved: [u8; 427],             // zero
}

impl VhdFooter {
    pub fn new_fixed(disk_size: u64) -> Self {
        // CHS heuristic (spec): 16 heads, 63 sectors/track
        let heads = 16u8;
        let spt = 63u8;
        let total_sectors = disk_size / 512;
        let cyls = (total_sectors / (heads as u64 * spt as u64)) as u16;

        // Timestamp: seconds since 2000-01-01
        let epoch_2000 = OffsetDateTime::from_unix_timestamp(946684800).unwrap();
        let now = OffsetDateTime::now_utc();
        let seconds_since_2000 = (now - epoch_2000).whole_seconds() as u32;

        let unique_id = *uuid::Uuid::new_v4().as_bytes();

        let mut f = VhdFooter {
            cookie: *b"conectix",
            features: U32::new(0x0000_0002),
            file_format_ver: U32::new(0x0001_0000),
            data_offset: U64::new(0xFFFF_FFFF_FFFF_FFFF),
            timestamp: U32::new(seconds_since_2000),
            creator_app: *b"rim\0",
            creator_ver: U32::new(0x000A_0000),
            creator_os: *b"Wi2k",
            orig_size: U64::new(disk_size),
            curr_size: U64::new(disk_size),
            geometry_cyls: cyls.to_be_bytes(),
            geometry_heads: heads,
            geometry_sects: spt,
            disk_type: U32::new(2),
            checksum: U32::new(0), // zeroed before calculation
            unique_id,
            saved_state: 0,
            reserved: [0u8; 427],
        };
        // injecte le checksum
        let sum = f.compute_checksum();
        f.checksum = U32::new(sum);
        f
    }

    /// Calculates the ones' complement of the sum of the 512 bytes with checksum=0.
    pub fn compute_checksum(&self) -> u32 {
        let mut tmp = *self;
        tmp.checksum = U32::new(0);
        let bytes: &[u8; 512] = tmp.as_bytes().try_into().unwrap();
        let mut sum: u32 = 0;
        for &b in bytes.iter() {
            sum = sum.wrapping_add(b as u32);
        }
        !sum
    }

    /// Validates the cookie, size and checksum.
    #[allow(dead_code)]
    pub fn validate(&self) -> bool {
        if &self.cookie != b"conectix" {
            return false;
        }
        // checksum
        let mut tmp = *self;
        let expected = tmp.checksum.get();
        tmp.checksum = U32::new(0);
        let bytes: &[u8; 512] = tmp.as_bytes().try_into().unwrap();
        let mut sum: u32 = 0;
        for &b in bytes.iter() {
            sum = sum.wrapping_add(b as u32);
        }
        (!sum) == expected
    }
}

const VHD_FOOTER_SIZE: u64 = 512;

pub fn create(
    layout: &Layout,
    output: &Path,
    truncate: &bool,
    dry_mode: DryRunMode,
) -> anyhow::Result<()> {
    crate::log_verbose!("Create temp img.");
    let temp_root = tempfile::tempdir()?;
    let temp_path = temp_root.path().join("rim_temp.img");
    img::create(layout, &temp_path, truncate, dry_mode)?;
    if matches!(dry_mode, DryRunMode::Off) {
        crate::log_verbose!("Wrapping img to vhd.");
        wrap_raw_as_vhd_to(&temp_path, output)?;
        return Ok(());
    }
    crate::log_verbose!("Dry-run - Wrapping img to vhd.");
    Ok(())
}

/// Strip VHD footer and restore .img
#[allow(dead_code)]
pub fn unwrap_vhd_to_raw(vhd_path: &Path, img_path: &Path) -> anyhow::Result<()> {
    let mut file = File::open(vhd_path)?;
    let len = file.metadata()?.len();
    let raw_len = len - VHD_FOOTER_SIZE;
    let mut buffer = vec![0u8; raw_len as usize];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut buffer)?;
    fs::write(img_path, &buffer)?;
    Ok(())
}

/// Wrap .img as a temporary .vhd (adds 512B VHD footer)
pub fn wrap_raw_as_vhd_to(img_path: &Path, vhd_path: &Path) -> anyhow::Result<()> {
    let mut reader = BufReader::new(File::open(img_path)?);
    let mut writer = BufWriter::new(File::create(vhd_path)?);

    let img_len = std::fs::metadata(img_path)?.len();
    let mut total_size = crate::utils::progress::copy_with_progress(
        &mut reader,
        &mut writer,
        img_len,
        "Converting to VHD",
    )?;

    let remainder = total_size % VHD_FOOTER_SIZE;
    if remainder != 0 {
        let pad = vec![0u8; (VHD_FOOTER_SIZE - remainder) as usize];
        writer.write_all(&pad)?;
        total_size += VHD_FOOTER_SIZE - remainder;
    }

    let footer = VhdFooter::new_fixed(total_size);
    writer.write_all(footer.as_bytes())?;
    writer.flush()?;
    drop(writer);
    Ok(())
}
