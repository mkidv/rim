// SPDX-License-Identifier: MIT

//! VHD (Virtual Hard Disk) fixed format support.
//!
//! Implements Microsoft VHD Fixed Disk format (`disk_type = 2`).

use rimio::prelude::*;
use zerocopy::byteorder::{BigEndian, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::errors::{RimImgError, RimImgResult};
use crate::options::ImageOptions;

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
    pub fn new_fixed(disk_size: u64, options: ImageOptions) -> Self {
        // CHS heuristic (spec): 16 heads, 63 sectors/track
        let heads = 16u8;
        let spt = 63u8;
        let total_sectors = disk_size / 512;
        let cyls = (total_sectors / (heads as u64 * spt as u64)) as u16;

        let mut f = VhdFooter {
            cookie: *b"conectix",
            features: U32::new(0x0000_0002),
            file_format_ver: U32::new(0x0001_0000),
            data_offset: U64::new(0xFFFF_FFFF_FFFF_FFFF),
            timestamp: U32::new(options.timestamp_seconds),
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
            unique_id: options.unique_id,
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

    /// Validates the cookie, checksum, and disk type (fixed disk = 2).
    pub fn validate(&self) -> bool {
        if &self.cookie != b"conectix" {
            return false;
        }
        if self.disk_type.get() != 2 {
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

pub fn wrap_raw_as_vhd_io_with_progress<F: FnMut(u64, u64)>(
    src: &mut dyn RimRead,
    dst: &mut dyn RimIO,
    img_len: u64,
    options: ImageOptions,
    on_progress: F,
) -> RimImgResult {
    dst.copy_from_with_progress(src, 0, 0, img_len, on_progress)?;
    let mut total_size = img_len;

    let remainder = total_size % VHD_FOOTER_SIZE;
    if remainder != 0 {
        let pad_size =
            usize::try_from(VHD_FOOTER_SIZE - remainder).map_err(|_| RimImgError::SizeOverflow)?;
        dst.zero_fill(total_size, pad_size)?;
        total_size += pad_size as u64;
    }

    let footer = VhdFooter::new_fixed(total_size, options);
    dst.write_struct(total_size, &footer)?;
    dst.flush()?;

    Ok(())
}

pub fn wrap_raw_as_vhd_io(
    src: &mut dyn RimRead,
    dst: &mut dyn RimIO,
    img_len: u64,
    options: ImageOptions,
) -> RimImgResult {
    wrap_raw_as_vhd_io_with_progress(src, dst, img_len, options, |_, _| {})
}

pub fn unwrap_vhd_io_with_progress<F: FnMut(u64, u64)>(
    src: &mut dyn RimIO,
    dst: &mut dyn RimWrite,
    on_progress: F,
) -> RimImgResult {
    let vhd_len = src.total_size()?;
    if vhd_len < VHD_FOOTER_SIZE {
        return Err(RimImgError::InvalidHeader(
            "VHD file too small (less than 512 bytes)",
        ));
    }

    let footer: VhdFooter = src.read_struct(vhd_len - VHD_FOOTER_SIZE)?;
    if &footer.cookie != b"conectix" {
        return Err(RimImgError::InvalidHeader("Invalid VHD cookie"));
    }
    if footer.compute_checksum() != footer.checksum.get() {
        return Err(RimImgError::Corrupted("Invalid VHD footer checksum"));
    }
    if footer.disk_type.get() != 2 {
        return Err(RimImgError::UnsupportedFormat);
    }

    let raw_len = vhd_len - VHD_FOOTER_SIZE;
    dst.copy_from_with_progress(src, 0, 0, raw_len, on_progress)?;
    dst.flush()?;

    Ok(())
}

pub fn unwrap_vhd_io(src: &mut dyn RimIO, dst: &mut dyn RimWrite) -> RimImgResult {
    unwrap_vhd_io_with_progress(src, dst, |_, _| {})
}

const _: () = {
    assert!(core::mem::size_of::<VhdFooter>() == 512);
    assert!(core::mem::align_of::<VhdFooter>() == 1);
    assert!(core::mem::offset_of!(VhdFooter, features) == 8);
    assert!(core::mem::offset_of!(VhdFooter, data_offset) == 16);
    assert!(core::mem::offset_of!(VhdFooter, timestamp) == 24);
    assert!(core::mem::offset_of!(VhdFooter, orig_size) == 40);
    assert!(core::mem::offset_of!(VhdFooter, curr_size) == 48);
    assert!(core::mem::offset_of!(VhdFooter, geometry_cyls) == 56);
    assert!(core::mem::offset_of!(VhdFooter, disk_type) == 60);
    assert!(core::mem::offset_of!(VhdFooter, checksum) == 64);
    assert!(core::mem::offset_of!(VhdFooter, unique_id) == 68);
    assert!(core::mem::offset_of!(VhdFooter, saved_state) == 84);
    assert!(core::mem::offset_of!(VhdFooter, reserved) == 85);
};
