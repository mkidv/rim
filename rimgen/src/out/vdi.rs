// SPDX-License-Identifier: MIT

//! VDI (VirtualBox Disk Image) fixed format support.
//!
//! This implements VDI fixed/preallocated format - the simplest variant
//! with a header followed by raw disk data.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::layout::Layout;
use crate::out::img;
use crate::out::target::DryRunMode;

use zerocopy::byteorder::{LittleEndian, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// VDI signature
const VDI_SIGNATURE: u32 = 0xbeda107f;

/// VDI version 1.1 (major.minor as 0x00010001)
const VDI_VERSION: u32 = 0x00010001;

/// Image type: Fixed
const VDI_TYPE_FIXED: u32 = 2;

/// Block size: 1MB
const BLOCK_SIZE: u32 = 1024 * 1024;

/// Offset where data starts (header + block map, aligned to 1MB)
const DATA_OFFSET: u64 = 1024 * 1024;

/// VDI pre-header (64 bytes) - contains file info text
#[repr(C)]
#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Clone, Copy)]
pub struct VdiPreHeader {
    pub file_info: [u8; 64], // "<<< Oracle VM VirtualBox Disk Image >>>\n"
}

/// VDI header (version 1.1, 400 bytes without pre-header)
#[repr(C)]
#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Clone, Copy)]
pub struct VdiHeader {
    pub signature: U32<LittleEndian>,     // 0xbeda107f
    pub version: U32<LittleEndian>,       // 0x00010001
    pub header_size: U32<LittleEndian>,   // Size of this header (400)
    pub image_type: U32<LittleEndian>,    // 2 = fixed
    pub image_flags: U32<LittleEndian>,   // 0
    pub description: [u8; 256],           // Image description
    pub offset_blocks: U32<LittleEndian>, // Offset to block map
    pub offset_data: U32<LittleEndian>,   // Offset to data
    pub geometry_cylinders: U32<LittleEndian>,
    pub geometry_heads: U32<LittleEndian>,
    pub geometry_sectors: U32<LittleEndian>,
    pub sector_size: U32<LittleEndian>, // 512
    pub unused1: U32<LittleEndian>,
    pub disk_size: U64<LittleEndian>,        // Virtual disk size
    pub block_size: U32<LittleEndian>,       // 1MB
    pub block_extra_data: U32<LittleEndian>, // 0
    pub blocks_in_image: U32<LittleEndian>,  // Total blocks
    pub blocks_allocated: U32<LittleEndian>, // Allocated blocks (all for fixed)
    pub uuid_image: [u8; 16],                // Image UUID
    pub uuid_last_snap: [u8; 16],            // Last snapshot UUID
    pub uuid_link: [u8; 16],                 // Link UUID
    pub uuid_parent: [u8; 16],               // Parent UUID
    pub unused2: [u8; 56],                   // Padding to 400 bytes
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
            offset_blocks: U32::new(512), // Block map after pre-header + header
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
        crate::log_verbose!("Wrapping img to vdi.");
        wrap_raw_as_vdi_to(&temp_path, output)?;
        return Ok(());
    }
    crate::log_verbose!("Dry-run - Wrapping img to vdi.");
    Ok(())
}

/// Wrap a raw .img file as VDI (fixed)
pub fn wrap_raw_as_vdi_to(img_path: &Path, vdi_path: &Path) -> anyhow::Result<()> {
    let img_size = std::fs::metadata(img_path)?.len();
    let num_blocks = img_size.div_ceil(BLOCK_SIZE as u64) as u32;

    let mut writer = BufWriter::new(File::create(vdi_path)?);

    // Write pre-header (64 bytes)
    let mut pre_header = VdiPreHeader {
        file_info: [0u8; 64],
    };
    let info_text = b"<<< Oracle VM VirtualBox Disk Image >>>\n";
    pre_header.file_info[..info_text.len()].copy_from_slice(info_text);
    writer.write_all(pre_header.as_bytes())?;

    // Write header
    let uuid = uuid::Uuid::new_v4();
    let header = VdiHeader::new_fixed(img_size, *uuid.as_bytes());
    writer.write_all(header.as_bytes())?;

    // Padding after header (64 + 400 = 464, need to reach 512 for block map)
    let header_total = 64 + 400;
    let padding_to_blocks = 512 - header_total;
    writer.write_all(&vec![0u8; padding_to_blocks])?;

    // Write block map (each entry is 4 bytes, value = block index)
    // For fixed disks, block N maps to physical block N
    let block_map_size = (num_blocks * 4) as usize;
    let mut block_map = Vec::with_capacity(block_map_size);
    for i in 0..num_blocks {
        block_map.extend_from_slice(&i.to_le_bytes());
    }
    writer.write_all(&block_map)?;

    // Pad to DATA_OFFSET
    let current_pos = 512 + block_map_size as u64;
    if current_pos < DATA_OFFSET {
        let padding = (DATA_OFFSET - current_pos) as usize;
        writer.write_all(&vec![0u8; padding])?;
    }

    // Copy raw data
    let mut reader = BufReader::new(File::open(img_path)?);
    crate::utils::progress::copy_with_progress(
        &mut reader,
        &mut writer,
        img_size,
        "Converting to VDI",
    )?;

    writer.flush()?;
    Ok(())
}

/// Strip VDI header and restore raw .img
#[allow(dead_code)]
pub fn unwrap_vdi_to_raw(vdi_path: &Path, img_path: &Path) -> anyhow::Result<()> {
    let mut file = File::open(vdi_path)?;
    let len = file.metadata()?.len();

    // Read header to get disk size
    file.seek(SeekFrom::Start(64))?; // Skip pre-header
    let mut header_bytes = [0u8; 400];
    file.read_exact(&mut header_bytes)?;
    let header = VdiHeader::read_from_bytes(&header_bytes)
        .map_err(|_| anyhow::anyhow!("Invalid VDI header"))?;

    let disk_size = header.disk_size.get();
    let data_offset = header.offset_data.get() as u64;

    file.seek(SeekFrom::Start(data_offset))?;

    let read_size = (len - data_offset).min(disk_size);
    let mut buffer = vec![0u8; read_size as usize];
    file.read_exact(&mut buffer)?;
    std::fs::write(img_path, &buffer)?;
    Ok(())
}
