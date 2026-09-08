// SPDX-License-Identifier: MIT

//! VDI (VirtualBox Disk Image) fixed format support.
//!
//! Implements VDI Fixed format (version 1.1) with pre-header, header, block map,
//! and 1MB data blocks.

#[cfg(feature = "alloc")]
use alloc::vec::Vec;
use rimio::prelude::*;
use zerocopy::byteorder::{LittleEndian, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::errors::{RimImgError, RimImgResult};
#[cfg(feature = "alloc")]
use crate::options::ImageOptions;

pub const VDI_SIGNATURE: u32 = 0xbeda107f;
pub const VDI_VERSION: u32 = 0x00010001;
pub const VDI_TYPE_FIXED: u32 = 2;
pub const BLOCK_SIZE: u32 = 1024 * 1024; // 1MB
pub const DATA_OFFSET: u64 = 1024 * 1024; // 1MB

#[repr(C)]
#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Clone, Copy, Debug)]
pub struct VdiPreHeader {
    pub file_info: [u8; 64], // "<<< Oracle VM VirtualBox Disk Image >>>\n"
}

#[repr(C)]
#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Clone, Copy, Debug)]
pub struct VdiHeader {
    pub signature: U32<LittleEndian>,
    pub version: U32<LittleEndian>,
    pub header_size: U32<LittleEndian>, // 400
    pub image_type: U32<LittleEndian>,  // 2 = fixed
    pub image_flags: U32<LittleEndian>, // 0
    pub description: [u8; 256],
    pub offset_blocks: U32<LittleEndian>, // 512
    pub offset_data: U32<LittleEndian>,   // 1MB
    pub geometry_cylinders: U32<LittleEndian>,
    pub geometry_heads: U32<LittleEndian>,
    pub geometry_sectors: U32<LittleEndian>,
    pub sector_size: U32<LittleEndian>, // 512
    pub unused1: U32<LittleEndian>,
    pub disk_size: U64<LittleEndian>,
    pub block_size: U32<LittleEndian>, // 1MB
    pub block_extra_data: U32<LittleEndian>,
    pub blocks_in_image: U32<LittleEndian>,
    pub blocks_allocated: U32<LittleEndian>,
    pub uuid_image: [u8; 16],
    pub uuid_last_snap: [u8; 16],
    pub uuid_link: [u8; 16],
    pub uuid_parent: [u8; 16],
    pub unused2: [u8; 56],
}

/// Calculate data offset dynamically aligned to 1MB boundary based on block map size.
pub fn calculate_data_offset(disk_size: u64) -> u64 {
    let blocks = disk_size.div_ceil(BLOCK_SIZE as u64);
    let bmap_bytes = blocks.saturating_mul(4);
    let bmap_end = 512u64.saturating_add(bmap_bytes);
    let rem = bmap_end % (BLOCK_SIZE as u64);
    if rem == 0 {
        bmap_end
    } else {
        bmap_end.saturating_add((BLOCK_SIZE as u64) - rem)
    }
}

impl VdiHeader {
    pub fn new_fixed(disk_size: u64, uuid: [u8; 16]) -> Self {
        let blocks = disk_size.div_ceil(BLOCK_SIZE as u64) as u32;
        let total_sectors = disk_size / 512;
        let cylinders = (total_sectors / (16 * 63)) as u32;
        let data_offset = calculate_data_offset(disk_size);

        Self {
            signature: U32::new(VDI_SIGNATURE),
            version: U32::new(VDI_VERSION),
            header_size: U32::new(400),
            image_type: U32::new(VDI_TYPE_FIXED),
            image_flags: U32::new(0),
            description: [0u8; 256],
            offset_blocks: U32::new(512),
            offset_data: U32::new(data_offset as u32),
            geometry_cylinders: U32::new(cylinders),
            geometry_heads: U32::new(16),
            geometry_sectors: U32::new(63),
            sector_size: U32::new(512),
            unused1: U32::new(0),
            disk_size: U64::new(disk_size),
            block_size: U32::new(BLOCK_SIZE),
            block_extra_data: U32::new(0),
            blocks_in_image: U32::new(blocks),
            blocks_allocated: U32::new(blocks),
            uuid_image: uuid,
            uuid_last_snap: [0u8; 16],
            uuid_link: [0u8; 16],
            uuid_parent: [0u8; 16],
            unused2: [0u8; 56],
        }
    }
}

#[cfg(feature = "alloc")]
pub fn wrap_raw_as_vdi_io_with_progress<F: FnMut(u64, u64)>(
    src: &mut dyn RimRead,
    dst: &mut dyn RimIO,
    img_len: u64,
    options: ImageOptions,
    on_progress: F,
) -> RimImgResult {
    let data_offset = init_vdi_io(dst, img_len, options)?;
    dst.copy_from_with_progress(src, 0, data_offset, img_len, on_progress)?;
    dst.flush()?;

    Ok(())
}

#[cfg(feature = "alloc")]
pub fn init_vdi_io(dst: &mut dyn RimIO, img_len: u64, options: ImageOptions) -> RimImgResult<u64> {
    let num_blocks = u32::try_from(img_len.div_ceil(BLOCK_SIZE as u64))
        .map_err(|_| RimImgError::SizeOverflow)?;

    let mut pre_header = VdiPreHeader {
        file_info: [0u8; 64],
    };
    let info_text = b"<<< Oracle VM VirtualBox Disk Image >>>\n";
    pre_header.file_info[..info_text.len()].copy_from_slice(info_text);
    dst.write_struct(0, &pre_header)?;

    let header = VdiHeader::new_fixed(img_len, options.unique_id);
    let data_offset = header.offset_data.get() as u64;
    dst.write_struct(64, &header)?;

    let header_total = 64 + 400;
    dst.zero_fill(header_total, 512 - header_total as usize)?;

    let block_map_size = usize::try_from(num_blocks).map_err(|_| RimImgError::SizeOverflow)? * 4;
    let mut block_map = Vec::with_capacity(block_map_size);
    for i in 0..num_blocks {
        block_map.extend_from_slice(&i.to_le_bytes());
    }
    dst.write_at(512, &block_map)?;

    let current_pos = 512 + block_map_size as u64;
    if current_pos < data_offset {
        let pad_size =
            usize::try_from(data_offset - current_pos).map_err(|_| RimImgError::SizeOverflow)?;
        dst.zero_fill(current_pos, pad_size)?;
    }

    Ok(data_offset)
}

#[cfg(feature = "alloc")]
pub fn wrap_raw_as_vdi_io(
    src: &mut dyn RimRead,
    dst: &mut dyn RimIO,
    img_len: u64,
    options: ImageOptions,
) -> RimImgResult {
    wrap_raw_as_vdi_io_with_progress(src, dst, img_len, options, |_, _| {})
}

pub fn unwrap_vdi_io_with_progress<F: FnMut(u64, u64)>(
    src: &mut dyn RimIO,
    dst: &mut dyn RimWrite,
    on_progress: F,
) -> RimImgResult {
    let header: VdiHeader = src.read_struct(64)?;
    validate_vdi_header(&header)?;

    let disk_size = header.disk_size.get();
    let data_offset = header.offset_data.get() as u64;

    dst.copy_from_with_progress(src, data_offset, 0, disk_size, on_progress)?;
    dst.flush()?;

    Ok(())
}

pub fn unwrap_vdi_io(src: &mut dyn RimIO, dst: &mut dyn RimWrite) -> RimImgResult {
    unwrap_vdi_io_with_progress(src, dst, |_, _| {})
}

pub fn validate_vdi_header(header: &VdiHeader) -> RimImgResult {
    if header.signature.get() != VDI_SIGNATURE {
        return Err(RimImgError::InvalidHeader("Invalid VDI signature"));
    }
    if header.version.get() != VDI_VERSION {
        return Err(RimImgError::UnsupportedFormat);
    }
    if header.image_type.get() != VDI_TYPE_FIXED {
        return Err(RimImgError::UnsupportedFormat);
    }
    let offset_bmap = header.offset_blocks.get() as u64;
    let bmap_bytes = (header.blocks_in_image.get() as u64)
        .checked_mul(4)
        .ok_or(RimImgError::SizeOverflow)?;
    let bmap_end = offset_bmap
        .checked_add(bmap_bytes)
        .ok_or(RimImgError::SizeOverflow)?;
    if (header.offset_data.get() as u64) < bmap_end {
        return Err(RimImgError::Corrupted("VDI data offset overlaps block map"));
    }
    Ok(())
}
