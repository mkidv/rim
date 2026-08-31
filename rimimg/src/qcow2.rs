// SPDX-License-Identifier: MIT

//! QCOW2 (QEMU Copy-On-Write v2) format support.
//!
//! Implements a linear flat QCOW2 v2 format (64KB clusters) with 1:1 mapping,
//! no compression, encryption, or snapshots.

#[cfg(feature = "alloc")]
use alloc::vec;
use rimio::prelude::*;
use zerocopy::byteorder::{BigEndian, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::errors::{RimImgError, RimImgResult};

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

#[cfg(feature = "alloc")]
pub fn wrap_raw_as_qcow2_io_with_progress<F: FnMut(u64, u64)>(
    src: &mut dyn RimRead,
    dst: &mut dyn RimIO,
    img_len: u64,
    on_progress: F,
) -> RimImgResult {
    let (virtual_size, data_start) = init_qcow2_layout(dst, img_len)?;

    dst.copy_from_with_progress(src, 0, data_start, img_len, on_progress)?;

    if img_len < virtual_size {
        let pad_size =
            usize::try_from(virtual_size - img_len).map_err(|_| RimImgError::SizeOverflow)?;
        dst.zero_fill(data_start + img_len, pad_size)?;
    }

    dst.flush()?;
    Ok(())
}

#[cfg(feature = "alloc")]
pub fn init_qcow2_io(dst: &mut dyn RimIO, img_len: u64) -> RimImgResult<u64> {
    let (_, data_start) = init_qcow2_layout(dst, img_len)?;
    Ok(data_start)
}

#[cfg(feature = "alloc")]
fn init_qcow2_layout(dst: &mut dyn RimIO, img_len: u64) -> RimImgResult<(u64, u64)> {
    let virtual_size = img_len.div_ceil(CLUSTER_SIZE) * CLUSTER_SIZE;
    let num_clusters = virtual_size / CLUSTER_SIZE;

    let l2_entries = CLUSTER_SIZE / 8;
    let l1_size =
        u32::try_from(num_clusters.div_ceil(l2_entries)).map_err(|_| RimImgError::SizeOverflow)?;

    let refcount_table_offset = CLUSTER_SIZE;
    let refcount_block_offset = CLUSTER_SIZE * 2;
    let l1_offset = CLUSTER_SIZE * 3;
    let l2_start = CLUSTER_SIZE * 4;
    let data_start = l2_start + (l1_size as u64) * CLUSTER_SIZE;

    let header = Qcow2Header::new(virtual_size, l1_size, l1_offset, refcount_table_offset, 1);
    dst.write_struct(0, &header)?;

    dst.zero_fill(
        core::mem::size_of::<Qcow2Header>() as u64,
        CLUSTER_SIZE as usize - core::mem::size_of::<Qcow2Header>(),
    )?;

    let mut refcount_table = vec![0u8; CLUSTER_SIZE as usize];
    refcount_table[0..8].copy_from_slice(&refcount_block_offset.to_be_bytes());
    dst.write_at(refcount_table_offset, &refcount_table)?;

    let metadata_clusters = 4 + l1_size as u64;
    let total_clusters = metadata_clusters + num_clusters;
    let mut refcount_block = vec![0u8; CLUSTER_SIZE as usize];
    for i in 0..total_clusters.min(CLUSTER_SIZE / 2) {
        let offset = (i * 2) as usize;
        if offset + 1 < refcount_block.len() {
            refcount_block[offset..offset + 2].copy_from_slice(&1u16.to_be_bytes());
        }
    }
    dst.write_at(refcount_block_offset, &refcount_block)?;

    let mut l1_table = vec![0u8; CLUSTER_SIZE as usize];
    for i in 0..l1_size {
        let l2_offset = l2_start + (i as u64) * CLUSTER_SIZE;
        let entry = l2_offset | (1u64 << 63);
        let offset = (i * 8) as usize;
        l1_table[offset..offset + 8].copy_from_slice(&entry.to_be_bytes());
    }
    dst.write_at(l1_offset, &l1_table)?;

    for l1_idx in 0..l1_size {
        let mut l2_table = vec![0u8; CLUSTER_SIZE as usize];
        for l2_idx in 0..l2_entries {
            let cluster_idx = (l1_idx as u64) * l2_entries + l2_idx;
            if cluster_idx < num_clusters {
                let data_offset = data_start + cluster_idx * CLUSTER_SIZE;
                let entry = data_offset | (1u64 << 63);
                let offset = (l2_idx * 8) as usize;
                l2_table[offset..offset + 8].copy_from_slice(&entry.to_be_bytes());
            }
        }
        dst.write_at(l2_start + (l1_idx as u64) * CLUSTER_SIZE, &l2_table)?;
    }

    dst.flush()?;
    Ok((virtual_size, data_start))
}

#[cfg(feature = "alloc")]
pub fn wrap_raw_as_qcow2_io(
    src: &mut dyn RimRead,
    dst: &mut dyn RimIO,
    img_len: u64,
) -> RimImgResult {
    wrap_raw_as_qcow2_io_with_progress(src, dst, img_len, |_, _| {})
}

pub fn unwrap_qcow2_io_with_progress<F: FnMut(u64, u64)>(
    src: &mut dyn RimIO,
    dst: &mut dyn RimWrite,
    on_progress: F,
) -> RimImgResult {
    let header: Qcow2Header = src.read_struct(0)?;

    validate_qcow2_header(&header)?;

    let virtual_size = header.size.get();
    let data_start = data_start_from_header(&header);

    dst.copy_from_with_progress(src, data_start, 0, virtual_size, on_progress)?;
    dst.flush()?;

    Ok(())
}

pub fn unwrap_qcow2_io(src: &mut dyn RimIO, dst: &mut dyn RimWrite) -> RimImgResult {
    unwrap_qcow2_io_with_progress(src, dst, |_, _| {})
}

pub fn validate_qcow2_header(header: &Qcow2Header) -> RimImgResult {
    if header.magic.get() != QCOW2_MAGIC {
        return Err(RimImgError::InvalidHeader("Invalid QCOW2 magic"));
    }
    if header.version.get() != QCOW2_VERSION
        || header.cluster_bits.get() != CLUSTER_BITS
        || header.backing_file_size.get() != 0
        || header.crypt_method.get() != 0
        || header.nb_snapshots.get() != 0
    {
        return Err(RimImgError::UnsupportedFormat);
    }
    Ok(())
}

pub fn data_start_from_header(header: &Qcow2Header) -> u64 {
    CLUSTER_SIZE * 4 + (header.l1_size.get() as u64) * CLUSTER_SIZE
}
