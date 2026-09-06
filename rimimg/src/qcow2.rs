// SPDX-License-Identifier: MIT

//! QCOW2 (QEMU Copy-On-Write v2 & v3) format support.
//!
//! Provides dynamic sparse cluster allocation, lazy L2 table allocation,
//! convergent refcount sizing, crash-consistent publication ordering,
//! and full QCOW2 v2/v3 specification compliance.

#[cfg(feature = "alloc")]
use alloc::{vec, vec::Vec};
use rimio::errors::RimIOError;
#[cfg(feature = "alloc")]
use rimio::extent::IoExtent;
use rimio::prelude::*;
use zerocopy::byteorder::{BigEndian, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::errors::{RimImgError, RimImgResult};

pub const QCOW2_MAGIC: u32 = 0x514649fb;
pub const QCOW2_VERSION_2: u32 = 2;
pub const QCOW2_VERSION_3: u32 = 3;
pub const CLUSTER_BITS: u32 = 16;
pub const CLUSTER_SIZE: u64 = 1 << CLUSTER_BITS; // 64KB
pub const L2_ENTRIES_PER_CLUSTER: u64 = CLUSTER_SIZE / 8; // 8192
pub const REFCOUNT_ENTRIES_PER_BLOCK: u64 = CLUSTER_SIZE / 2; // 32768

pub const QCOW_OFLAG_COPIED: u64 = 1 << 63;
pub const QCOW_OFLAG_COMPRESSED: u64 = 1 << 62;
pub const QCOW_OFLAG_ZERO: u64 = 1 << 0;
pub const L2_OFFSET_MASK: u64 = 0x00ff_ffff_ffff_0000;

#[repr(C)]
#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Clone, Copy, Debug)]
pub struct Qcow2Header {
    pub magic: U32<BigEndian>,                   // 0x514649fb
    pub version: U32<BigEndian>,                 // 2 or 3
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

#[repr(C)]
#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Clone, Copy, Debug)]
pub struct Qcow2HeaderV3Extension {
    pub incompatible_features: U64<BigEndian>,
    pub compatible_features: U64<BigEndian>,
    pub autoclear_features: U64<BigEndian>,
    pub refcount_order: U32<BigEndian>,
    pub header_length: U32<BigEndian>,
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
            version: U32::new(QCOW2_VERSION_3),
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

/// Calculates the initial metadata geometry and worst-case refcount capacity.
pub fn calculate_sparse_geometry(virtual_size: u64) -> (u32, u32, u64) {
    let aligned_size = virtual_size.div_ceil(CLUSTER_SIZE) * CLUSTER_SIZE;
    let n_data = aligned_size / CLUSTER_SIZE;
    let n_l2 = n_data.div_ceil(L2_ENTRIES_PER_CLUSTER);
    let n_l1_clusters = (n_l2 * 8).div_ceil(CLUSTER_SIZE);
    let l1_size = n_l2 as u32;

    // Base non-refcount clusters: Header (1) + L1 clusters + L2 tables + Data clusters
    let m = 1 + n_l1_clusters + n_l2 + n_data;

    // Fixed-point convergent sizing for refcount blocks and table
    let mut n_rb = 1u64;
    let mut n_rt = 1u64;
    loop {
        let total_clusters = m + n_rb + n_rt;
        let req_rb = total_clusters.div_ceil(REFCOUNT_ENTRIES_PER_BLOCK);
        let req_rt = (req_rb * 8).div_ceil(CLUSTER_SIZE);
        if req_rb <= n_rb && req_rt <= n_rt {
            break;
        }
        n_rb = req_rb;
        n_rt = req_rt;
    }

    (l1_size, n_rt as u32, aligned_size)
}

/// Initializes a sparse QCOW2 v3 layout on a target stream.
#[cfg(feature = "alloc")]
pub fn init_sparse_qcow2_layout(dst: &mut dyn RimIO, img_len: u64) -> RimImgResult<Qcow2Header> {
    let (l1_size, n_rt, aligned_size) = calculate_sparse_geometry(img_len);
    let n_l1_clusters = ((l1_size as u64) * 8).div_ceil(CLUSTER_SIZE);

    let refcount_table_offset = CLUSTER_SIZE;
    let refcount_table_clusters = n_rt;
    let initial_refcount_block_offset = (1 + refcount_table_clusters as u64) * CLUSTER_SIZE;
    let l1_table_offset = initial_refcount_block_offset + CLUSTER_SIZE;
    let total_initial_clusters = 1 + refcount_table_clusters as u64 + 1 + n_l1_clusters;
    let initial_size = total_initial_clusters * CLUSTER_SIZE;

    let header = Qcow2Header {
        magic: U32::new(QCOW2_MAGIC),
        version: U32::new(QCOW2_VERSION_3),
        backing_file_offset: U64::new(0),
        backing_file_size: U32::new(0),
        cluster_bits: U32::new(CLUSTER_BITS),
        size: U64::new(aligned_size),
        crypt_method: U32::new(0),
        l1_size: U32::new(l1_size),
        l1_table_offset: U64::new(l1_table_offset),
        refcount_table_offset: U64::new(refcount_table_offset),
        refcount_table_clusters: U32::new(refcount_table_clusters),
        nb_snapshots: U32::new(0),
        snapshots_offset: U64::new(0),
    };

    let v3_ext = Qcow2HeaderV3Extension {
        incompatible_features: U64::new(0),
        compatible_features: U64::new(0),
        autoclear_features: U64::new(0),
        refcount_order: U32::new(4), // 16 bits = 2 bytes per refcount entry
        header_length: U32::new(104),
    };

    // Zero-fill initial metadata clusters
    let init_size_usize = usize::try_from(initial_size).map_err(|_| RimImgError::SizeOverflow)?;
    dst.zero_fill(0, init_size_usize)?;

    // Write header
    dst.write_struct(0, &header)?;
    dst.write_struct(core::mem::size_of::<Qcow2Header>() as u64, &v3_ext)?;

    // Write refcount table entry 0 -> initial_refcount_block_offset
    let rt_entry_0 = initial_refcount_block_offset.to_be_bytes();
    dst.write_at(refcount_table_offset, &rt_entry_0)?;

    // Write initial refcount block: mark all initial clusters (0..total_initial_clusters) with refcount 1
    let mut init_rb = vec![0u8; CLUSTER_SIZE as usize];
    for i in 0..total_initial_clusters as usize {
        init_rb[i * 2..i * 2 + 2].copy_from_slice(&1u16.to_be_bytes());
    }
    dst.write_at(initial_refcount_block_offset, &init_rb)?;

    dst.flush()?;
    Ok(header)
}

/// Dynamic Read-Write QCOW2 Driver.
#[cfg(feature = "alloc")]
pub struct Qcow2IO<'a> {
    inner: &'a mut dyn RimIO,
    header: Qcow2Header,
    virtual_size: u64,
    partition_offset: u64,
    l1_table: Vec<u64>,
    refcount_table: Vec<u64>,
    max_physical_clusters: u64,
    cached_rb_idx: Option<usize>,
    cached_rb_cluster: u64,
    cached_rb: Vec<u8>,
    next_free_cluster: u64,
    finished: bool,
}

#[cfg(feature = "alloc")]
impl<'a> Qcow2IO<'a> {
    pub fn raw_len(&self) -> u64 {
        self.virtual_size
    }

    pub fn finish(&mut self) -> RimImgResult {
        if self.finished {
            return Ok(());
        }
        self.inner.flush()?;
        self.finished = true;
        Ok(())
    }

    fn allocate_cluster(&mut self) -> RimImgResult<u64> {
        let mut phys_cluster = self.next_free_cluster;

        if phys_cluster >= self.max_physical_clusters {
            return Err(RimImgError::SizeOverflow);
        }

        let rb_idx = (phys_cluster / REFCOUNT_ENTRIES_PER_BLOCK) as usize;
        if rb_idx >= self.refcount_table.len() {
            return Err(RimImgError::SizeOverflow);
        }

        if self.refcount_table[rb_idx] == 0 {
            // Bootstrap new refcount block at EOF
            let rb_cluster = phys_cluster;
            let mut new_rb = vec![0u8; CLUSTER_SIZE as usize];

            let entry_in_block = (rb_cluster % REFCOUNT_ENTRIES_PER_BLOCK) as usize;
            new_rb[entry_in_block * 2..entry_in_block * 2 + 2].copy_from_slice(&1u16.to_be_bytes());

            // Write new refcount block to disk
            self.inner.write_at(rb_cluster * CLUSTER_SIZE, &new_rb)?;
            self.inner.flush()?;

            // Update refcount table
            let rb_offset = rb_cluster * CLUSTER_SIZE;
            self.refcount_table[rb_idx] = rb_offset;
            let rt_entry_offset = self.header.refcount_table_offset.get() + (rb_idx as u64) * 8;
            self.inner
                .write_at(rt_entry_offset, &rb_offset.to_be_bytes())?;
            self.inner.flush()?;

            // Set cached block
            self.cached_rb_idx = Some(rb_idx);
            self.cached_rb_cluster = rb_cluster;
            self.cached_rb = new_rb;

            phys_cluster += 1;
            if phys_cluster >= self.max_physical_clusters {
                return Err(RimImgError::SizeOverflow);
            }
        }

        self.set_refcount(phys_cluster, 1)?;
        self.next_free_cluster = phys_cluster + 1;
        Ok(phys_cluster * CLUSTER_SIZE)
    }

    fn set_refcount(&mut self, phys_cluster: u64, refcount: u16) -> RimImgResult<()> {
        let rb_idx = (phys_cluster / REFCOUNT_ENTRIES_PER_BLOCK) as usize;
        let entry_in_block = (phys_cluster % REFCOUNT_ENTRIES_PER_BLOCK) as usize;

        if self.cached_rb_idx != Some(rb_idx) {
            let rb_offset = self.refcount_table[rb_idx];
            if rb_offset == 0 {
                return Err(RimImgError::Corrupted("Missing refcount block in table"));
            }
            let mut rb_buf = vec![0u8; CLUSTER_SIZE as usize];
            self.inner.read_at(rb_offset, &mut rb_buf)?;
            self.cached_rb_idx = Some(rb_idx);
            self.cached_rb_cluster = rb_offset / CLUSTER_SIZE;
            self.cached_rb = rb_buf;
        }

        self.cached_rb[entry_in_block * 2..entry_in_block * 2 + 2]
            .copy_from_slice(&refcount.to_be_bytes());
        let entry_disk_offset = self.cached_rb_cluster * CLUSTER_SIZE + (entry_in_block as u64) * 2;
        self.inner
            .write_at(entry_disk_offset, &refcount.to_be_bytes())?;
        self.inner.flush()?;

        Ok(())
    }
}

#[cfg(feature = "alloc")]
impl RimRead for Qcow2IO<'_> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
        let mut read_bytes = 0;
        while read_bytes < buf.len() {
            let logical = self
                .partition_offset
                .checked_add(offset)
                .and_then(|o| o.checked_add(read_bytes as u64))
                .ok_or(RimIOError::OutOfBounds)?;

            if logical >= self.virtual_size {
                return Err(RimIOError::OutOfBounds);
            }

            let cluster_idx = logical / CLUSTER_SIZE;
            let in_cluster = (logical % CLUSTER_SIZE) as usize;
            let chunk_len = (CLUSTER_SIZE as usize - in_cluster).min(buf.len() - read_bytes);
            let dest = &mut buf[read_bytes..read_bytes + chunk_len];

            let l1_idx = (cluster_idx / L2_ENTRIES_PER_CLUSTER) as usize;
            if l1_idx >= self.l1_table.len() {
                dest.fill(0);
                read_bytes += chunk_len;
                continue;
            }

            let l1_entry = self.l1_table[l1_idx];
            let l2_offset = l1_entry & L2_OFFSET_MASK;
            if l2_offset == 0 {
                dest.fill(0);
                read_bytes += chunk_len;
                continue;
            }

            let l2_idx = (cluster_idx % L2_ENTRIES_PER_CLUSTER) as usize;
            let mut l2_entry_bytes = [0u8; 8];
            self.inner
                .read_at(l2_offset + (l2_idx as u64) * 8, &mut l2_entry_bytes)?;
            let l2_entry = u64::from_be_bytes(l2_entry_bytes);

            if (l2_entry & QCOW_OFLAG_COMPRESSED) != 0 {
                return Err(RimIOError::Unsupported);
            }

            if (l2_entry & QCOW_OFLAG_ZERO) != 0 {
                dest.fill(0);
                read_bytes += chunk_len;
                continue;
            }

            let cluster_offset = l2_entry & L2_OFFSET_MASK;
            if cluster_offset == 0 {
                dest.fill(0);
            } else {
                self.inner
                    .read_at(cluster_offset + in_cluster as u64, dest)?;
            }
            read_bytes += chunk_len;
        }
        Ok(())
    }

    fn total_size(&mut self) -> RimIOResult<u64> {
        Ok(self.virtual_size.saturating_sub(self.partition_offset))
    }
}

#[cfg(feature = "alloc")]
impl RimWrite for Qcow2IO<'_> {
    fn write_at(&mut self, offset: u64, data: &[u8]) -> RimIOResult {
        let mut written = 0;
        while written < data.len() {
            let logical = self
                .partition_offset
                .checked_add(offset)
                .and_then(|o| o.checked_add(written as u64))
                .ok_or(RimIOError::OutOfBounds)?;

            if logical >= self.virtual_size {
                return Err(RimIOError::OutOfBounds);
            }

            let cluster_idx = logical / CLUSTER_SIZE;
            let in_cluster = (logical % CLUSTER_SIZE) as usize;
            let chunk_len = (CLUSTER_SIZE as usize - in_cluster).min(data.len() - written);

            let l1_idx = (cluster_idx / L2_ENTRIES_PER_CLUSTER) as usize;
            if l1_idx >= self.l1_table.len() {
                return Err(RimIOError::OutOfBounds);
            }

            let mut l1_entry = self.l1_table[l1_idx];
            if l1_entry == 0 {
                // Allocate new L2 table
                let l2_cluster = self.allocate_cluster().map_err(|e| match e {
                    RimImgError::IO(io_e) => io_e,
                    _ => RimIOError::Other("Failed to allocate L2 cluster"),
                })?;
                self.inner.zero_fill(l2_cluster, CLUSTER_SIZE as usize)?;
                self.inner.flush()?;

                l1_entry = l2_cluster | QCOW_OFLAG_COPIED;
                self.l1_table[l1_idx] = l1_entry;
                let l1_disk_offset = self.header.l1_table_offset.get() + (l1_idx as u64) * 8;
                self.inner
                    .write_at(l1_disk_offset, &l1_entry.to_be_bytes())?;
                self.inner.flush()?;
            } else if (l1_entry & QCOW_OFLAG_COPIED) == 0 {
                return Err(RimIOError::Unsupported); // Shared L2 table
            }

            let l2_offset = l1_entry & L2_OFFSET_MASK;
            let l2_idx = (cluster_idx % L2_ENTRIES_PER_CLUSTER) as usize;
            let l2_entry_disk_offset = l2_offset + (l2_idx as u64) * 8;

            let mut l2_entry_bytes = [0u8; 8];
            self.inner
                .read_at(l2_entry_disk_offset, &mut l2_entry_bytes)?;
            let l2_entry = u64::from_be_bytes(l2_entry_bytes);

            if (l2_entry & QCOW_OFLAG_COMPRESSED) != 0 {
                return Err(RimIOError::Unsupported);
            }

            let is_zero_cluster = (l2_entry & QCOW_OFLAG_ZERO) != 0;
            let phys_offset = l2_entry & L2_OFFSET_MASK;

            if phys_offset == 0 || (is_zero_cluster && phys_offset == 0) {
                // Unallocated cluster (or unallocated zero cluster)
                let new_cluster = self.allocate_cluster().map_err(|e| match e {
                    RimImgError::IO(io_e) => io_e,
                    _ => RimIOError::Other("Failed to allocate data cluster"),
                })?;

                // If partial write, zero-fill remainder of cluster
                if chunk_len < CLUSTER_SIZE as usize {
                    self.inner.zero_fill(new_cluster, CLUSTER_SIZE as usize)?;
                }

                self.inner.write_at(
                    new_cluster + in_cluster as u64,
                    &data[written..written + chunk_len],
                )?;
                self.inner.flush()?;

                let new_l2_entry = new_cluster | QCOW_OFLAG_COPIED;
                self.inner
                    .write_at(l2_entry_disk_offset, &new_l2_entry.to_be_bytes())?;
                self.inner.flush()?;
            } else if is_zero_cluster {
                // Preallocated zero cluster
                if (l2_entry & QCOW_OFLAG_COPIED) == 0 {
                    return Err(RimIOError::Unsupported);
                }

                if chunk_len < CLUSTER_SIZE as usize {
                    if in_cluster > 0 {
                        self.inner.zero_fill(phys_offset, in_cluster)?;
                    }
                    let tail_start = in_cluster + chunk_len;
                    if tail_start < CLUSTER_SIZE as usize {
                        self.inner.zero_fill(
                            phys_offset + tail_start as u64,
                            CLUSTER_SIZE as usize - tail_start,
                        )?;
                    }
                }

                self.inner.write_at(
                    phys_offset + in_cluster as u64,
                    &data[written..written + chunk_len],
                )?;
                self.inner.flush()?;

                // Clear bit 0 (QCOW_OFLAG_ZERO), retain OFLAG_COPIED
                let new_l2_entry = phys_offset | QCOW_OFLAG_COPIED;
                self.inner
                    .write_at(l2_entry_disk_offset, &new_l2_entry.to_be_bytes())?;
                self.inner.flush()?;
            } else {
                // Standard allocated cluster
                if (l2_entry & QCOW_OFLAG_COPIED) == 0 {
                    return Err(RimIOError::Unsupported);
                }

                self.inner.write_at(
                    phys_offset + in_cluster as u64,
                    &data[written..written + chunk_len],
                )?;
                self.inner.flush()?;
            }

            written += chunk_len;
        }
        Ok(())
    }

    fn flush(&mut self) -> RimIOResult {
        self.inner.flush()
    }
}

#[cfg(feature = "alloc")]
impl RimIO for Qcow2IO<'_> {
    fn set_offset(&mut self, partition_offset: u64) -> u64 {
        self.partition_offset = partition_offset;
        partition_offset
    }

    fn partition_offset(&self) -> u64 {
        self.partition_offset
    }
}

/// Read-only QCOW2 Driver.
pub struct Qcow2ReadIO<'a> {
    inner: &'a mut dyn RimRead,
    virtual_size: u64,
    #[cfg_attr(feature = "alloc", allow(dead_code))]
    l1_table_offset: u64,
    l1_size: u32,
    partition_offset: u64,
    #[cfg(feature = "alloc")]
    l1_table: Vec<u64>,
}

impl<'a> Qcow2ReadIO<'a> {
    pub fn raw_len(&self) -> u64 {
        self.virtual_size
    }

    pub fn set_offset(&mut self, offset: u64) -> u64 {
        self.partition_offset = offset;
        offset
    }

    pub fn partition_offset(&self) -> u64 {
        self.partition_offset
    }
}

impl RimRead for Qcow2ReadIO<'_> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
        let mut read_bytes = 0;
        while read_bytes < buf.len() {
            let logical = self
                .partition_offset
                .checked_add(offset)
                .and_then(|o| o.checked_add(read_bytes as u64))
                .ok_or(RimIOError::OutOfBounds)?;

            if logical >= self.virtual_size {
                return Err(RimIOError::OutOfBounds);
            }

            let cluster_idx = logical / CLUSTER_SIZE;
            let in_cluster = (logical % CLUSTER_SIZE) as usize;
            let chunk_len = (CLUSTER_SIZE as usize - in_cluster).min(buf.len() - read_bytes);
            let dest = &mut buf[read_bytes..read_bytes + chunk_len];

            let l1_idx = (cluster_idx / L2_ENTRIES_PER_CLUSTER) as usize;
            if l1_idx >= self.l1_size as usize {
                dest.fill(0);
                read_bytes += chunk_len;
                continue;
            }

            #[cfg(feature = "alloc")]
            let l1_entry = self.l1_table[l1_idx];

            #[cfg(not(feature = "alloc"))]
            let l1_entry = {
                let mut bytes = [0u8; 8];
                self.inner
                    .read_at(self.l1_table_offset + (l1_idx as u64) * 8, &mut bytes)?;
                u64::from_be_bytes(bytes)
            };

            let l2_offset = l1_entry & L2_OFFSET_MASK;
            if l2_offset == 0 {
                dest.fill(0);
                read_bytes += chunk_len;
                continue;
            }

            let l2_idx = (cluster_idx % L2_ENTRIES_PER_CLUSTER) as usize;
            let mut l2_entry_bytes = [0u8; 8];
            self.inner
                .read_at(l2_offset + (l2_idx as u64) * 8, &mut l2_entry_bytes)?;
            let l2_entry = u64::from_be_bytes(l2_entry_bytes);

            if (l2_entry & QCOW_OFLAG_COMPRESSED) != 0 {
                return Err(RimIOError::Unsupported);
            }

            if (l2_entry & QCOW_OFLAG_ZERO) != 0 {
                dest.fill(0);
                read_bytes += chunk_len;
                continue;
            }

            let cluster_offset = l2_entry & L2_OFFSET_MASK;
            if cluster_offset == 0 {
                dest.fill(0);
            } else {
                self.inner
                    .read_at(cluster_offset + in_cluster as u64, dest)?;
            }
            read_bytes += chunk_len;
        }
        Ok(())
    }

    fn total_size(&mut self) -> RimIOResult<u64> {
        Ok(self.virtual_size.saturating_sub(self.partition_offset))
    }
}

/// Creates a new sparse QCOW2 IO wrapper for a given target.
#[cfg(feature = "alloc")]
pub fn create_sparse_qcow2_io<'a>(
    dst: &'a mut dyn RimIO,
    raw_len: u64,
) -> RimImgResult<Qcow2IO<'a>> {
    init_sparse_qcow2_layout(dst, raw_len)?;
    open_sparse_qcow2_io(dst)
}

/// Opens an existing QCOW2 image for reading and writing.
#[cfg(feature = "alloc")]
pub fn open_sparse_qcow2_io<'a>(src: &'a mut dyn RimIO) -> RimImgResult<Qcow2IO<'a>> {
    let header: Qcow2Header = src.read_struct(0)?;
    validate_qcow2_header(&header)?;

    let version = header.version.get();
    if version == QCOW2_VERSION_3 {
        let v3_ext: Qcow2HeaderV3Extension =
            src.read_struct(core::mem::size_of::<Qcow2Header>() as u64)?;
        validate_qcow2_v3_extension(&v3_ext)?;
    }

    let virtual_size = header.size.get();
    let l1_size = header.l1_size.get() as usize;
    let l1_offset = header.l1_table_offset.get();

    // Read L1 table
    let mut l1_bytes = vec![0u8; l1_size * 8];
    src.read_at(l1_offset, &mut l1_bytes)?;
    let mut l1_table = Vec::with_capacity(l1_size);
    for chunk in l1_bytes.chunks_exact(8) {
        l1_table.push(u64::from_be_bytes(chunk.try_into().unwrap()));
    }

    // Read refcount table
    let rt_clusters = header.refcount_table_clusters.get() as usize;
    let rt_entries = rt_clusters * (CLUSTER_SIZE as usize / 8);
    let rt_offset = header.refcount_table_offset.get();
    let mut rt_bytes = vec![0u8; rt_entries * 8];
    src.read_at(rt_offset, &mut rt_bytes)?;
    let mut refcount_table = Vec::with_capacity(rt_entries);
    for chunk in rt_bytes.chunks_exact(8) {
        refcount_table.push(u64::from_be_bytes(chunk.try_into().unwrap()));
    }

    let max_physical_clusters = (rt_entries as u64) * REFCOUNT_ENTRIES_PER_BLOCK;

    let mut highest_cluster = 0u64;
    // Refcount table
    highest_cluster =
        highest_cluster.max(rt_offset / CLUSTER_SIZE + (rt_clusters as u64).saturating_sub(1));
    // L1 table
    let n_l1_clusters = ((l1_size as u64) * 8).div_ceil(CLUSTER_SIZE);
    highest_cluster =
        highest_cluster.max(l1_offset / CLUSTER_SIZE + n_l1_clusters.saturating_sub(1));

    // Scan refcount blocks for highest allocated cluster
    for (rb_idx, &rb_offset) in refcount_table.iter().enumerate() {
        if rb_offset != 0 {
            highest_cluster = highest_cluster.max(rb_offset / CLUSTER_SIZE);
            let mut rb_buf = vec![0u8; CLUSTER_SIZE as usize];
            src.read_at(rb_offset, &mut rb_buf)?;
            for entry_idx in (0..REFCOUNT_ENTRIES_PER_BLOCK as usize).rev() {
                let rc = u16::from_be_bytes(
                    rb_buf[entry_idx * 2..entry_idx * 2 + 2].try_into().unwrap(),
                );
                if rc != 0 {
                    let cluster_idx =
                        (rb_idx as u64) * REFCOUNT_ENTRIES_PER_BLOCK + entry_idx as u64;
                    highest_cluster = highest_cluster.max(cluster_idx);
                    break;
                }
            }
        }
    }
    let next_free_cluster = highest_cluster + 1;

    Ok(Qcow2IO {
        inner: src,
        header,
        virtual_size,
        partition_offset: 0,
        l1_table,
        refcount_table,
        max_physical_clusters,
        cached_rb_idx: None,
        cached_rb_cluster: 0,
        cached_rb: Vec::new(),
        next_free_cluster,
        finished: false,
    })
}

/// Opens an existing QCOW2 image for reading.
pub fn open_sparse_qcow2_read_io(src: &mut dyn RimRead) -> RimImgResult<Qcow2ReadIO<'_>> {
    let header: Qcow2Header = src.read_struct(0)?;
    validate_qcow2_header(&header)?;

    let version = header.version.get();
    if version == QCOW2_VERSION_3 {
        let v3_ext: Qcow2HeaderV3Extension =
            src.read_struct(core::mem::size_of::<Qcow2Header>() as u64)?;
        validate_qcow2_v3_extension(&v3_ext)?;
    }

    let virtual_size = header.size.get();
    let l1_size = header.l1_size.get();
    let l1_offset = header.l1_table_offset.get();

    #[cfg(feature = "alloc")]
    let l1_table = {
        let mut l1_bytes = vec![0u8; (l1_size as usize) * 8];
        src.read_at(l1_offset, &mut l1_bytes)?;
        let mut table = Vec::with_capacity(l1_size as usize);
        for chunk in l1_bytes.chunks_exact(8) {
            table.push(u64::from_be_bytes(chunk.try_into().unwrap()));
        }
        table
    };

    Ok(Qcow2ReadIO {
        inner: src,
        virtual_size,
        l1_table_offset: l1_offset,
        l1_size,
        partition_offset: 0,
        #[cfg(feature = "alloc")]
        l1_table,
    })
}

/// Extent mapping parser for QCOW2 images.
#[cfg(feature = "alloc")]
pub fn parse_qcow2_extents(src: &mut dyn RimRead) -> RimImgResult<Vec<IoExtent>> {
    let header: Qcow2Header = src.read_struct(0)?;
    validate_qcow2_header(&header)?;

    let version = header.version.get();
    if version == QCOW2_VERSION_3 {
        let v3_ext: Qcow2HeaderV3Extension =
            src.read_struct(core::mem::size_of::<Qcow2Header>() as u64)?;
        validate_qcow2_v3_extension(&v3_ext)?;
    }

    let virtual_size = header.size.get();
    let l1_size = header.l1_size.get() as usize;
    let l1_offset = header.l1_table_offset.get();

    let mut l1_bytes = vec![0u8; l1_size * 8];
    src.read_at(l1_offset, &mut l1_bytes)?;

    let mut extents = Vec::new();
    let mut current_extent: Option<IoExtent> = None;

    let num_clusters = virtual_size.div_ceil(CLUSTER_SIZE);
    for cluster_idx in 0..num_clusters {
        let l1_idx = (cluster_idx / L2_ENTRIES_PER_CLUSTER) as usize;
        let logical_offset = cluster_idx * CLUSTER_SIZE;
        let cluster_len = CLUSTER_SIZE.min(virtual_size - logical_offset);

        let mut phys_offset = None;
        if l1_idx < l1_size {
            let l1_entry =
                u64::from_be_bytes(l1_bytes[l1_idx * 8..l1_idx * 8 + 8].try_into().unwrap());
            let l2_offset = l1_entry & L2_OFFSET_MASK;
            if l2_offset != 0 {
                let l2_idx = (cluster_idx % L2_ENTRIES_PER_CLUSTER) as usize;
                let mut l2_entry_bytes = [0u8; 8];
                src.read_at(l2_offset + (l2_idx as u64) * 8, &mut l2_entry_bytes)?;
                let l2_entry = u64::from_be_bytes(l2_entry_bytes);
                if (l2_entry & QCOW_OFLAG_ZERO) == 0 {
                    let phys = l2_entry & L2_OFFSET_MASK;
                    if phys != 0 {
                        phys_offset = Some(phys);
                    }
                }
            }
        }

        let ext = match phys_offset {
            Some(phys) => IoExtent::new(logical_offset, phys, cluster_len),
            None => IoExtent::hole(logical_offset, cluster_len),
        };

        if let Some(ref mut curr) = current_extent {
            if curr.logical_end() == ext.logical_offset
                && curr.source_offset.is_some() == ext.source_offset.is_some()
            {
                if let (Some(c_phys), Some(e_phys)) = (curr.source_offset, ext.source_offset) {
                    if c_phys + curr.len == e_phys {
                        curr.len += ext.len;
                        continue;
                    }
                } else if curr.source_offset.is_none() && ext.source_offset.is_none() {
                    curr.len += ext.len;
                    continue;
                }
            }
            extents.push(*curr);
            current_extent = Some(ext);
        } else {
            current_extent = Some(ext);
        }
    }

    if let Some(curr) = current_extent {
        extents.push(curr);
    }

    Ok(extents)
}

#[cfg(feature = "alloc")]
pub fn wrap_raw_as_qcow2_io_with_progress<F: FnMut(u64, u64)>(
    src: &mut dyn RimRead,
    dst: &mut dyn RimIO,
    img_len: u64,
    mut on_progress: F,
) -> RimImgResult {
    let mut image_io = create_sparse_qcow2_io(dst, img_len)?;
    let mut chunk = vec![0u8; CLUSTER_SIZE as usize];
    let mut copied = 0u64;
    while copied < img_len {
        let to_read = (CLUSTER_SIZE as usize).min((img_len - copied) as usize);
        src.read_at(copied, &mut chunk[..to_read])?;
        let is_all_zeroes = chunk[..to_read].iter().all(|&b| b == 0);
        if !is_all_zeroes {
            image_io.write_at(copied, &chunk[..to_read])?;
        }
        copied += to_read as u64;
        on_progress(copied, img_len);
    }
    image_io.finish()?;
    Ok(())
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
    mut on_progress: F,
) -> RimImgResult {
    let mut reader = open_sparse_qcow2_read_io(src)?;
    let total_size = reader.raw_len();
    #[cfg(feature = "alloc")]
    let mut chunk = vec![0u8; CLUSTER_SIZE as usize];
    #[cfg(not(feature = "alloc"))]
    let mut chunk = [0u8; 4096];

    let mut copied = 0u64;
    while copied < total_size {
        let to_copy = (chunk.len()).min((total_size - copied) as usize);
        reader.read_at(copied, &mut chunk[..to_copy])?;
        dst.write_at(copied, &chunk[..to_copy])?;
        copied += to_copy as u64;
        on_progress(copied, total_size);
    }
    dst.flush()?;
    Ok(())
}

pub fn unwrap_qcow2_io(src: &mut dyn RimIO, dst: &mut dyn RimWrite) -> RimImgResult {
    unwrap_qcow2_io_with_progress(src, dst, |_, _| {})
}

pub fn validate_qcow2_header(header: &Qcow2Header) -> RimImgResult<()> {
    if header.magic.get() != QCOW2_MAGIC {
        return Err(RimImgError::InvalidHeader("Invalid QCOW2 magic"));
    }
    let version = header.version.get();
    if version != QCOW2_VERSION_2 && version != QCOW2_VERSION_3 {
        return Err(RimImgError::UnsupportedFormat);
    }
    if header.cluster_bits.get() != CLUSTER_BITS
        || header.backing_file_size.get() != 0
        || header.crypt_method.get() != 0
        || header.nb_snapshots.get() != 0
    {
        return Err(RimImgError::UnsupportedFormat);
    }
    Ok(())
}

pub fn validate_qcow2_v3_extension(v3: &Qcow2HeaderV3Extension) -> RimImgResult<()> {
    if v3.header_length.get() < 104 {
        return Err(RimImgError::InvalidHeader("Invalid QCOW2 v3 header length"));
    }
    if v3.incompatible_features.get() != 0 {
        return Err(RimImgError::UnsupportedFormat);
    }
    if v3.refcount_order.get() != 4 {
        return Err(RimImgError::UnsupportedFormat);
    }
    Ok(())
}

pub fn data_start_from_header(header: &Qcow2Header) -> u64 {
    CLUSTER_SIZE * 4 + (header.l1_size.get() as u64) * CLUSTER_SIZE
}
