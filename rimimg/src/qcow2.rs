// SPDX-License-Identifier: MIT

//! QCOW2 (QEMU Copy-On-Write v2) format support.
//!
//! Implements a linear flat QCOW2 v2 format (64KB clusters) with 1:1 mapping,
//! no compression, encryption, or snapshots.

use std::fs::File;
use std::path::Path;

use anyhow::Context;
use rimio::prelude::*;
use zerocopy::byteorder::{BigEndian, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

pub const QCOW2_MAGIC: u32 = 0x514649fb;
pub const QCOW2_VERSION: u32 = 2;
pub const CLUSTER_BITS: u32 = 16;
pub const CLUSTER_SIZE: u64 = 1 << CLUSTER_BITS; // 64KB

#[repr(C)]
#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Clone, Copy, Debug)]
pub struct Qcow2Header {
    pub magic: U32<BigEndian>,                   // 0x514649fb
    pub version: U32<BigEndian>,                 // 2
    pub backing_file_offset: U64<BigEndian>,     // 0
    pub backing_file_size: U32<BigEndian>,       // 0
    pub cluster_bits: U32<BigEndian>,            // 16 (64KB)
    pub size: U64<BigEndian>,                    // virtual disk size
    pub crypt_method: U32<BigEndian>,            // 0
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

/// Wrap a raw .img file as QCOW2 (flat v2) with progress callback.
pub fn wrap_raw_as_qcow2_with_progress<P1: AsRef<Path>, P2: AsRef<Path>, F: FnMut(u64, u64)>(
    img_path: P1,
    qcow2_path: P2,
    on_progress: F,
) -> anyhow::Result<()> {
    let mut img_file = File::open(img_path.as_ref())
        .with_context(|| format!("Failed to open input image {}", img_path.as_ref().display()))?;
    let img_len = img_file.metadata()?.len();

    let mut img_io = StdRimIO::new(&mut img_file);

    let virtual_size = img_len.div_ceil(CLUSTER_SIZE) * CLUSTER_SIZE;
    let num_clusters = virtual_size / CLUSTER_SIZE;

    let l2_entries = CLUSTER_SIZE / 8;
    let l1_size = num_clusters.div_ceil(l2_entries) as u32;

    let refcount_table_offset = CLUSTER_SIZE;
    let refcount_block_offset = CLUSTER_SIZE * 2;
    let l1_offset = CLUSTER_SIZE * 3;
    let l2_start = CLUSTER_SIZE * 4;
    let data_start = l2_start + (l1_size as u64) * CLUSTER_SIZE;

    let mut qcow2_file = File::create(qcow2_path.as_ref()).with_context(|| {
        format!(
            "Failed to create QCOW2 file {}",
            qcow2_path.as_ref().display()
        )
    })?;
    let mut qcow2_io = StdRimIO::new(&mut qcow2_file);

    // Write header
    let header = Qcow2Header::new(virtual_size, l1_size, l1_offset, refcount_table_offset, 1);
    qcow2_io.write_struct(0, &header)?;

    // Zero-fill padding for header cluster
    qcow2_io.zero_fill(
        std::mem::size_of::<Qcow2Header>() as u64,
        CLUSTER_SIZE as usize - std::mem::size_of::<Qcow2Header>(),
    )?;

    // Write refcount table (points to refcount block)
    let mut refcount_table = vec![0u8; CLUSTER_SIZE as usize];
    refcount_table[0..8].copy_from_slice(&refcount_block_offset.to_be_bytes());
    qcow2_io.write_at(refcount_table_offset, &refcount_table)?;

    // Write refcount block (mark all metadata clusters as used)
    let metadata_clusters = 4 + l1_size as u64;
    let total_clusters = metadata_clusters + num_clusters;
    let mut refcount_block = vec![0u8; CLUSTER_SIZE as usize];
    for i in 0..total_clusters.min(CLUSTER_SIZE / 2) {
        let offset = (i * 2) as usize;
        if offset + 1 < refcount_block.len() {
            refcount_block[offset..offset + 2].copy_from_slice(&1u16.to_be_bytes());
        }
    }
    qcow2_io.write_at(refcount_block_offset, &refcount_block)?;

    // Write L1 table (points to L2 tables)
    let mut l1_table = vec![0u8; CLUSTER_SIZE as usize];
    for i in 0..l1_size {
        let l2_offset = l2_start + (i as u64) * CLUSTER_SIZE;
        let entry = l2_offset | (1u64 << 63); // COPIED flag
        let offset = (i * 8) as usize;
        l1_table[offset..offset + 8].copy_from_slice(&entry.to_be_bytes());
    }
    qcow2_io.write_at(l1_offset, &l1_table)?;

    // Write L2 tables
    for l1_idx in 0..l1_size {
        let mut l2_table = vec![0u8; CLUSTER_SIZE as usize];
        for l2_idx in 0..l2_entries {
            let cluster_idx = (l1_idx as u64) * l2_entries + l2_idx;
            if cluster_idx < num_clusters {
                let data_offset = data_start + cluster_idx * CLUSTER_SIZE;
                let entry = data_offset | (1u64 << 63); // COPIED flag
                let offset = (l2_idx * 8) as usize;
                l2_table[offset..offset + 8].copy_from_slice(&entry.to_be_bytes());
            }
        }
        qcow2_io.write_at(l2_start + (l1_idx as u64) * CLUSTER_SIZE, &l2_table)?;
    }

    // Copy raw data
    qcow2_io.copy_from_with_progress(&mut img_io, 0, data_start, img_len, on_progress)?;

    // Pad to cluster boundary if needed
    let written = img_len;
    if written < virtual_size {
        qcow2_io.zero_fill(data_start + written, (virtual_size - written) as usize)?;
    }

    qcow2_io.flush()?;
    Ok(())
}

pub fn wrap_raw_as_qcow2_to<P1: AsRef<Path>, P2: AsRef<Path>>(
    img_path: P1,
    qcow2_path: P2,
) -> anyhow::Result<()> {
    wrap_raw_as_qcow2_with_progress(img_path, qcow2_path, |_, _| {})
}

/// Strip QCOW2 metadata and restore raw .img with progress callback.
pub fn unwrap_qcow2_with_progress<P1: AsRef<Path>, P2: AsRef<Path>, F: FnMut(u64, u64)>(
    qcow2_path: P1,
    img_path: P2,
    on_progress: F,
) -> anyhow::Result<()> {
    let mut qcow2_file = File::open(qcow2_path.as_ref()).with_context(|| {
        format!(
            "Failed to open QCOW2 file {}",
            qcow2_path.as_ref().display()
        )
    })?;
    let mut qcow2_io = StdRimIO::new(&mut qcow2_file);

    let header: Qcow2Header = qcow2_io
        .read_struct(0)
        .context("Failed to read QCOW2 header")?;

    let virtual_size = header.size.get();
    let l1_size = header.l1_size.get();

    let l2_start = CLUSTER_SIZE * 4;
    let data_start = l2_start + (l1_size as u64) * CLUSTER_SIZE;

    let mut img_file = File::create(img_path.as_ref())
        .with_context(|| format!("Failed to create raw image {}", img_path.as_ref().display()))?;
    let mut img_io = StdRimIO::new(&mut img_file);

    img_io.copy_from_with_progress(&mut qcow2_io, data_start, 0, virtual_size, on_progress)?;
    img_io.flush()?;

    Ok(())
}

pub fn unwrap_qcow2_to_raw<P1: AsRef<Path>, P2: AsRef<Path>>(
    qcow2_path: P1,
    img_path: P2,
) -> anyhow::Result<()> {
    unwrap_qcow2_with_progress(qcow2_path, img_path, |_, _| {})
}
