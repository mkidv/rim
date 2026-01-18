// SPDX-License-Identifier: MIT

//! QCOW2 (QEMU Copy-On-Write v2) format support.
//!
//! This implements a simplified QCOW2 format with no compression, encryption,
//! or snapshots - essentially a flat image with QCOW2 metadata.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::layout::Layout;
use crate::out::img;
use crate::out::target::DryRunMode;

use zerocopy::byteorder::{BigEndian, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// QCOW2 magic: "QFI\xfb"
const QCOW2_MAGIC: u32 = 0x514649fb;

/// QCOW2 version 2 (simpler, widely compatible)
const QCOW2_VERSION: u32 = 2;

/// Default cluster size: 64KB (cluster_bits = 16)
const CLUSTER_BITS: u32 = 16;
const CLUSTER_SIZE: u64 = 1 << CLUSTER_BITS;

/// QCOW2 header (version 2, 72 bytes)
#[repr(C)]
#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Clone, Copy)]
pub struct Qcow2Header {
    pub magic: U32<BigEndian>,                   // 0x514649fb
    pub version: U32<BigEndian>,                 // 2
    pub backing_file_offset: U64<BigEndian>,     // 0 (no backing file)
    pub backing_file_size: U32<BigEndian>,       // 0
    pub cluster_bits: U32<BigEndian>,            // 16 (64KB clusters)
    pub size: U64<BigEndian>,                    // virtual disk size
    pub crypt_method: U32<BigEndian>,            // 0 (no encryption)
    pub l1_size: U32<BigEndian>,                 // number of L1 entries
    pub l1_table_offset: U64<BigEndian>,         // offset to L1 table
    pub refcount_table_offset: U64<BigEndian>,   // offset to refcount table
    pub refcount_table_clusters: U32<BigEndian>, // clusters for refcount table
    pub nb_snapshots: U32<BigEndian>,            // 0
    pub snapshots_offset: U64<BigEndian>,        // 0
}

impl Qcow2Header {
    pub fn new(
        virtual_size: u64,
        l1_size: u32,
        l1_offset: u64,
        refcount_offset: u64,
        refcount_clusters: u32,
    ) -> Self {
        Self {
            magic: U32::new(QCOW2_MAGIC),
            version: U32::new(QCOW2_VERSION),
            backing_file_offset: U64::new(0),
            backing_file_size: U32::new(0),
            cluster_bits: U32::new(CLUSTER_BITS),
            size: U64::new(virtual_size),
            crypt_method: U32::new(0),
            l1_size: U32::new(l1_size),
            l1_table_offset: U64::new(l1_offset),
            refcount_table_offset: U64::new(refcount_offset),
            refcount_table_clusters: U32::new(refcount_clusters),
            nb_snapshots: U32::new(0),
            snapshots_offset: U64::new(0),
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
        crate::log_verbose!("Wrapping img to qcow2.");
        wrap_raw_as_qcow2_to(&temp_path, output)?;
        return Ok(());
    }
    crate::log_verbose!("Dry-run - Wrapping img to qcow2.");
    Ok(())
}

/// Wrap a raw .img file as QCOW2
pub fn wrap_raw_as_qcow2_to(img_path: &Path, qcow2_path: &Path) -> anyhow::Result<()> {
    let img_size = std::fs::metadata(img_path)?.len();

    // Round up to cluster boundary
    let virtual_size = img_size.div_ceil(CLUSTER_SIZE) * CLUSTER_SIZE;
    let num_clusters = virtual_size / CLUSTER_SIZE;

    // L2 entries per table (each entry is 8 bytes)
    let l2_entries = CLUSTER_SIZE / 8;
    // Number of L1 entries needed
    let l1_size = num_clusters.div_ceil(l2_entries) as u32;

    // Layout:
    // - Cluster 0: Header (padded to cluster)
    // - Cluster 1: Refcount table
    // - Cluster 2: Refcount block
    // - Cluster 3: L1 table
    // - Cluster 4+: L2 tables
    // - After L2: Data clusters

    let _header_offset = 0u64;
    let refcount_table_offset = CLUSTER_SIZE;
    let refcount_block_offset = CLUSTER_SIZE * 2;
    let l1_offset = CLUSTER_SIZE * 3;
    let l2_start = CLUSTER_SIZE * 4;
    let data_start = l2_start + (l1_size as u64) * CLUSTER_SIZE;

    let mut writer = BufWriter::new(File::create(qcow2_path)?);

    // Write header
    let header = Qcow2Header::new(virtual_size, l1_size, l1_offset, refcount_table_offset, 1);
    writer.write_all(header.as_bytes())?;

    // Pad header cluster
    let header_pad = CLUSTER_SIZE as usize - std::mem::size_of::<Qcow2Header>();
    writer.write_all(&vec![0u8; header_pad])?;

    // Write refcount table (points to refcount block)
    let mut refcount_table = vec![0u8; CLUSTER_SIZE as usize];
    refcount_table[0..8].copy_from_slice(&refcount_block_offset.to_be_bytes());
    writer.write_all(&refcount_table)?;

    // Write refcount block (mark all metadata clusters as used)
    let metadata_clusters = 4 + l1_size as u64; // header + refcount table + refcount block + L1 + L2s
    let total_clusters = metadata_clusters + num_clusters;
    let mut refcount_block = vec![0u8; CLUSTER_SIZE as usize];
    for i in 0..total_clusters.min(CLUSTER_SIZE / 2) {
        // Each refcount is 2 bytes (16-bit), set to 1
        let offset = (i * 2) as usize;
        if offset + 1 < refcount_block.len() {
            refcount_block[offset..offset + 2].copy_from_slice(&1u16.to_be_bytes());
        }
    }
    writer.write_all(&refcount_block)?;

    // Write L1 table (points to L2 tables)
    let mut l1_table = vec![0u8; CLUSTER_SIZE as usize];
    for i in 0..l1_size {
        let l2_offset = l2_start + (i as u64) * CLUSTER_SIZE;
        // L1 entry: offset with COPIED flag (bit 63)
        let entry = l2_offset | (1u64 << 63);
        let offset = (i * 8) as usize;
        l1_table[offset..offset + 8].copy_from_slice(&entry.to_be_bytes());
    }
    writer.write_all(&l1_table)?;

    // Write L2 tables
    for l1_idx in 0..l1_size {
        let mut l2_table = vec![0u8; CLUSTER_SIZE as usize];
        for l2_idx in 0..l2_entries {
            let cluster_idx = (l1_idx as u64) * l2_entries + l2_idx;
            if cluster_idx < num_clusters {
                let data_offset = data_start + cluster_idx * CLUSTER_SIZE;
                // L2 entry: offset with COPIED flag
                let entry = data_offset | (1u64 << 63);
                let offset = (l2_idx * 8) as usize;
                l2_table[offset..offset + 8].copy_from_slice(&entry.to_be_bytes());
            }
        }
        writer.write_all(&l2_table)?;
    }

    // Copy raw data
    let mut reader = BufReader::new(File::open(img_path)?);
    let written = crate::utils::progress::copy_with_progress(
        &mut reader,
        &mut writer,
        img_size,
        "Converting to QCOW2",
    )?;

    // Pad to cluster boundary
    if written < virtual_size {
        let padding = (virtual_size - written) as usize;
        writer.write_all(&vec![0u8; padding])?;
    }

    writer.flush()?;
    Ok(())
}

/// Strip QCOW2 metadata and restore raw .img
#[allow(dead_code)]
pub fn unwrap_qcow2_to_raw(qcow2_path: &Path, img_path: &Path) -> anyhow::Result<()> {
    let mut file = File::open(qcow2_path)?;

    // Read header
    let mut header_bytes = [0u8; 72];
    file.read_exact(&mut header_bytes)?;
    let header = Qcow2Header::read_from_bytes(&header_bytes)
        .map_err(|_| anyhow::anyhow!("Invalid QCOW2 header"))?;

    let virtual_size = header.size.get();
    let l1_size = header.l1_size.get();

    // Calculate data start (same layout as wrap)
    let l2_start = CLUSTER_SIZE * 4;
    let data_start = l2_start + (l1_size as u64) * CLUSTER_SIZE;

    file.seek(SeekFrom::Start(data_start))?;

    let mut buffer = vec![0u8; virtual_size as usize];
    file.read_exact(&mut buffer)?;
    std::fs::write(img_path, &buffer)?;
    Ok(())
}
