// SPDX-License-Identifier: MIT

//! VDI (VirtualBox Disk Image) fixed format support.
//!
//! Implements VDI Fixed format (version 1.1) with pre-header, header, block map,
//! and 1MB data blocks.

use std::fs::File;
use std::path::Path;

use anyhow::Context;
use rimio::prelude::*;
use zerocopy::byteorder::{LittleEndian, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

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

impl VdiHeader {
    pub fn new_fixed(disk_size: u64, uuid: [u8; 16]) -> Self {
        let blocks = disk_size.div_ceil(BLOCK_SIZE as u64) as u32;
        let total_sectors = disk_size / 512;
        let cylinders = (total_sectors / (16 * 63)) as u32;

        Self {
            signature: U32::new(VDI_SIGNATURE),
            version: U32::new(VDI_VERSION),
            header_size: U32::new(400),
            image_type: U32::new(VDI_TYPE_FIXED),
            image_flags: U32::new(0),
            description: [0u8; 256],
            offset_blocks: U32::new(512),
            offset_data: U32::new(DATA_OFFSET as u32),
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

/// Wrap a raw .img file as VDI (fixed) with progress callback.
pub fn wrap_raw_as_vdi_with_progress<P1: AsRef<Path>, P2: AsRef<Path>, F: FnMut(u64, u64)>(
    img_path: P1,
    vdi_path: P2,
    on_progress: F,
) -> anyhow::Result<()> {
    let mut img_file = File::open(img_path.as_ref())
        .with_context(|| format!("Failed to open input image {}", img_path.as_ref().display()))?;
    let img_len = img_file.metadata()?.len();

    let mut img_io = StdRimIO::new(&mut img_file);

    let num_blocks = img_len.div_ceil(BLOCK_SIZE as u64) as u32;

    let mut vdi_file = File::create(vdi_path.as_ref())
        .with_context(|| format!("Failed to create VDI file {}", vdi_path.as_ref().display()))?;
    let mut vdi_io = StdRimIO::new(&mut vdi_file);

    let mut pre_header = VdiPreHeader {
        file_info: [0u8; 64],
    };
    let info_text = b"<<< Oracle VM VirtualBox Disk Image >>>\n";
    pre_header.file_info[..info_text.len()].copy_from_slice(info_text);
    vdi_io.write_struct(0, &pre_header)?;

    let uuid = uuid::Uuid::new_v4();
    let header = VdiHeader::new_fixed(img_len, *uuid.as_bytes());
    vdi_io.write_struct(64, &header)?;

    let header_total = 64 + 400;
    vdi_io.zero_fill(header_total, (512 - header_total) as usize)?;

    let block_map_size = (num_blocks * 4) as usize;
    let mut block_map = Vec::with_capacity(block_map_size);
    for i in 0..num_blocks {
        block_map.extend_from_slice(&i.to_le_bytes());
    }
    vdi_io.write_at(512, &block_map)?;

    let current_pos = 512 + block_map_size as u64;
    if current_pos < DATA_OFFSET {
        vdi_io.zero_fill(current_pos, (DATA_OFFSET - current_pos) as usize)?;
    }

    vdi_io.copy_from_with_progress(&mut img_io, 0, DATA_OFFSET, img_len, on_progress)?;
    vdi_io.flush()?;

    Ok(())
}

pub fn wrap_raw_as_vdi_to<P1: AsRef<Path>, P2: AsRef<Path>>(
    img_path: P1,
    vdi_path: P2,
) -> anyhow::Result<()> {
    wrap_raw_as_vdi_with_progress(img_path, vdi_path, |_, _| {})
}

/// Strip VDI header and restore raw .img with progress callback.
pub fn unwrap_vdi_with_progress<P1: AsRef<Path>, P2: AsRef<Path>, F: FnMut(u64, u64)>(
    vdi_path: P1,
    img_path: P2,
    on_progress: F,
) -> anyhow::Result<()> {
    let mut vdi_file = File::open(vdi_path.as_ref())
        .with_context(|| format!("Failed to open VDI file {}", vdi_path.as_ref().display()))?;
    let mut vdi_io = StdRimIO::new(&mut vdi_file);

    let header: VdiHeader = vdi_io
        .read_struct(64)
        .context("Failed to read VDI header")?;
    let disk_size = header.disk_size.get();
    let data_offset = header.offset_data.get() as u64;

    let mut img_file = File::create(img_path.as_ref())
        .with_context(|| format!("Failed to create raw image {}", img_path.as_ref().display()))?;
    let mut img_io = StdRimIO::new(&mut img_file);

    img_io.copy_from_with_progress(&mut vdi_io, data_offset, 0, disk_size, on_progress)?;
    img_io.flush()?;

    Ok(())
}

pub fn unwrap_vdi_to_raw<P1: AsRef<Path>, P2: AsRef<Path>>(
    vdi_path: P1,
    img_path: P2,
) -> anyhow::Result<()> {
    unwrap_vdi_with_progress(vdi_path, img_path, |_, _| {})
}
