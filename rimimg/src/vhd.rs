// SPDX-License-Identifier: MIT

//! VHD (Virtual Hard Disk) fixed format support.
//!
//! Implements Microsoft VHD Fixed Disk format (`disk_type = 2`).

use std::fs::File;
use std::path::Path;

use anyhow::Context;
use rimio::prelude::*;
use time::OffsetDateTime;
use zerocopy::byteorder::{BigEndian, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

pub const VHD_FOOTER_SIZE: u64 = 512;

#[repr(C)]
#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Clone, Copy, Debug)]
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
        // inject the checksum
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

    /// Validates the cookie and checksum.
    pub fn validate(&self) -> bool {
        if &self.cookie != b"conectix" {
            return false;
        }
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

/// Wrap a raw .img file as a fixed .vhd with progress callback.
pub fn wrap_raw_as_vhd_with_progress<P1: AsRef<Path>, P2: AsRef<Path>, F: FnMut(u64, u64)>(
    img_path: P1,
    vhd_path: P2,
    on_progress: F,
) -> anyhow::Result<()> {
    let mut img_file = File::open(img_path.as_ref())
        .with_context(|| format!("Failed to open input image {}", img_path.as_ref().display()))?;
    let img_len = img_file.metadata()?.len();

    let mut img_io = StdRimIO::new(&mut img_file);

    let mut vhd_file = File::create(vhd_path.as_ref())
        .with_context(|| format!("Failed to create VHD file {}", vhd_path.as_ref().display()))?;
    let mut vhd_io = StdRimIO::new(&mut vhd_file);

    vhd_io.copy_from_with_progress(&mut img_io, 0, 0, img_len, on_progress)?;
    let mut total_size = img_len;

    let remainder = total_size % VHD_FOOTER_SIZE;
    if remainder != 0 {
        let pad_size = (VHD_FOOTER_SIZE - remainder) as usize;
        vhd_io.zero_fill(total_size, pad_size)?;
        total_size += pad_size as u64;
    }

    let footer = VhdFooter::new_fixed(total_size);
    vhd_io.write_struct(total_size, &footer)?;
    vhd_io.flush()?;

    Ok(())
}

pub fn wrap_raw_as_vhd_to<P1: AsRef<Path>, P2: AsRef<Path>>(
    img_path: P1,
    vhd_path: P2,
) -> anyhow::Result<()> {
    wrap_raw_as_vhd_with_progress(img_path, vhd_path, |_, _| {})
}

/// Strip VHD footer and restore raw .img with progress callback.
pub fn unwrap_vhd_with_progress<P1: AsRef<Path>, P2: AsRef<Path>, F: FnMut(u64, u64)>(
    vhd_path: P1,
    img_path: P2,
    on_progress: F,
) -> anyhow::Result<()> {
    let mut vhd_file = File::open(vhd_path.as_ref())
        .with_context(|| format!("Failed to open VHD file {}", vhd_path.as_ref().display()))?;
    let vhd_len = vhd_file.metadata()?.len();

    let mut vhd_io = StdRimIO::new(&mut vhd_file);
    if vhd_len < VHD_FOOTER_SIZE {
        anyhow::bail!("VHD file too small (less than 512 bytes)");
    }
    let raw_len = vhd_len - VHD_FOOTER_SIZE;

    let mut img_file = File::create(img_path.as_ref())
        .with_context(|| format!("Failed to create raw image {}", img_path.as_ref().display()))?;
    let mut img_io = StdRimIO::new(&mut img_file);

    img_io.copy_from_with_progress(&mut vhd_io, 0, 0, raw_len, on_progress)?;
    img_io.flush()?;

    Ok(())
}

pub fn unwrap_vhd_to_raw<P1: AsRef<Path>, P2: AsRef<Path>>(
    vhd_path: P1,
    img_path: P2,
) -> anyhow::Result<()> {
    unwrap_vhd_with_progress(vhd_path, img_path, |_, _| {})
}
