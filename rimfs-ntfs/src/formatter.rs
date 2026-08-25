// SPDX-License-Identifier: MIT
//! NTFS Formatter
//!
//! Writes the initial NTFS layout:
//! - Boot sector (VBR) + backup boot sector
//! - $MFT + $MFTMirr
//! - Core system files ($LogFile, $Volume, $AttrDef, $Root, $Bitmap, $Boot, $BadClus, $Secure, $UpCase, $Extend)
//! - $Extend children ($Quota, $ObjId, $Reparse) to avoid CHKDSK manufacturing them

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};

use rimio::prelude::MemRimIO;
use rimio::{RimIO, RimIOExt};

use crate::allocator::NtfsAllocator;
use crate::attrdef;
use crate::builder::system_files::sys_ref;
use crate::builder::{NtfsAttribute, NtfsMftRecord};
use crate::constant::*;
use crate::core::{FsFormatterError, FsFormatterResult, traits::FsFormatter};
use crate::flags::NtfsFileAttributes;
use crate::meta::NtfsMeta;
use crate::mft;
use crate::types::*;
use crate::upcase::UpcaseHandle;
use crate::utils::current_ntfs_time;
use crate::{FsAllocator, FsMeta};
use zerocopy::IntoBytes;

/// NTFS filesystem formatter
pub struct NtfsFormatter<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a NtfsMeta,
}

impl<'a, IO: RimIO + ?Sized> NtfsFormatter<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a NtfsMeta) -> Self {
        Self { io, meta }
    }

    // -------------------------
    // Helpers (no seq magic)
    // -------------------------

    #[inline]
    fn root_ref() -> u64 {
        sys_ref(MFT_RECORD_ROOT)
    }

    #[inline]
    fn extend_ref() -> u64 {
        sys_ref(MFT_RECORD_EXTEND)
    }

    fn encode_runs_to_dataruns(runs: &rimio::run::RunList) -> Vec<u8> {
        let mut out = Vec::new();
        let mut last_lcn = 0i64;

        for run in runs.iter() {
            let delta = run.start as i64 - last_lcn;
            let (enc, len) = crate::utils::encode_data_run(delta, run.length);
            out.extend_from_slice(&enc[..len]);
            last_lcn = run.start as i64;
        }
        out.push(crate::utils::encode_data_run_end());
        out
    }

    // -------------------------
    // Public entry
    // -------------------------

    pub fn format(&mut self, _full_format: bool) -> FsFormatterResult<()> {
        self.write_boot_sector()?;

        self.initialize_bitmap()?;
        let mut allocator = NtfsAllocator::new(self.meta)?;

        // Pre-zero MFT area
        let mft_offset = self.meta.mft_record_offset(0);
        let mft_area_size = self.meta.reserved_mft_records * self.meta.mft_record_size as u64;
        self.io
            .zero_fill(mft_offset, mft_area_size as usize)
            .map_err(crate::core::FsFormatterError::IO)?;

        self.write_system_mft_records(&mut allocator)?;
        self.write_mft_mirror()?; // final sync
        self.write_backup_boot_sector()?;

        Ok(())
    }

    // -------------------------
    // Boot sectors
    // -------------------------

    fn write_boot_sector(&mut self) -> FsFormatterResult<()> {
        let boot = NtfsBootSector::new_from_meta(self.meta);
        self.io.write_at(0, boot.as_bytes())?;
        Ok(())
    }

    fn write_backup_boot_sector(&mut self) -> FsFormatterResult<()> {
        // NTFS backup boot sector lives in the last sector of the volume.
        let last_sector_offset = (self.meta.total_sectors - 1) * self.meta.bytes_per_sector as u64;
        let mut boot_copy = vec![0u8; self.meta.bytes_per_sector as usize];
        self.io.read_at(0, &mut boot_copy)?;
        self.io.write_at(last_sector_offset, &boot_copy)?;
        Ok(())
    }

    // -------------------------
    // System records
    // -------------------------

    fn write_system_mft_records(
        &mut self,
        allocator: &mut NtfsAllocator<'a>,
    ) -> FsFormatterResult<()> {
        // Single synchronized timestamp across system files.
        let timestamp = current_ntfs_time();

        self.write_mft_record_mft(allocator)?;
        self.write_mft_record_mftmirr()?;
        self.write_mft_record_logfile()?;
        self.write_mft_record_volume()?;
        self.write_mft_record_attrdef(allocator)?;
        self.write_mft_record_root(allocator, timestamp)?;
        self.write_mft_record_bitmap()?;
        self.write_mft_record_boot()?;
        self.write_mft_record_badclus()?;
        self.write_mft_record_secure(allocator)?;
        self.write_mft_record_upcase()?;
        self.write_mft_record_extend(timestamp)?;

        // Create $Extend children to avoid CHKDSK creating them and mutating your FS.
        self.write_mft_record_quota(timestamp)?;
        self.write_mft_record_objid(timestamp)?;
        self.write_mft_record_reparse(timestamp)?;

        // Records 12-23: Free (IN_USE=false). IMPORTANT: do NOT mark them in-use.
        for i in MFT_RECORD_RESERVED_START..MFT_RECORD_USER_START {
            let record = NtfsMftRecord::new(i as u32, false, false);
            let raw = record.to_raw_buffer(self.meta)?;
            mft::write_record(self.io, self.meta, i, &raw)?;
        }

        // Mirror should reflect at least 0-3 (and often 0-3 only).
        self.write_mft_mirror()?;

        // Remaining reserved: write empty/free records (except ones we created).
        for i in MFT_RECORD_USER_START..NTFS_RESERVED_MFT_RECORDS {
            if i == MFT_RECORD_OBJID
                || i == MFT_RECORD_QUOTA
                || i == MFT_RECORD_REPARSE
                || i == MFT_RECORD_USNJRNL
            {
                continue;
            }
            self.write_empty_mft_record(i)?;
        }

        Ok(())
    }

    fn write_mft_mirror(&mut self) -> FsFormatterResult<()> {
        let mirror_offset = self.meta.lcn_to_offset(self.meta.mft_mirr_lcn);
        let mft_offset = self.meta.lcn_to_offset(self.meta.mft_lcn);

        // If cluster <= 4096, copy 4 records. Otherwise copy 1 cluster.
        let mirror_size = if self.meta.bytes_per_cluster <= 4096 {
            4 * self.meta.mft_record_size as usize
        } else {
            self.meta.bytes_per_cluster as usize
        };

        let mut buffer = vec![0u8; mirror_size];
        self.io.read_at(mft_offset, &mut buffer)?;
        self.io.write_at(mirror_offset, &buffer)?;
        Ok(())
    }

    // -------------------------
    // Volume cluster bitmap init ($Bitmap file data)
    // -------------------------

    fn initialize_bitmap(&mut self) -> FsFormatterResult<()> {
        use crate::core::bitmap::BitmapDriver;

        let mut driver = BitmapDriver::new(self.meta);

        // Clear bitmap (fill with 0)
        driver.format_with(self.io, 0)?;

        // 1) Reserve bootstrap + system region [0, first_data_unit)
        let first_data = self.meta.first_data_unit();
        driver.set_bits_range(self.io, 0, first_data, true)?;

        // 2) Reserve $MFT clusters
        let mft_clusters = self.meta.initial_mft_clusters();
        driver.set_bits_range(self.io, self.meta.mft_lcn, mft_clusters, true)?;

        // 3) Reserve $MFTMirr clusters
        let mirr_clusters =
            (4 * self.meta.mft_record_size as u64).div_ceil(self.meta.bytes_per_cluster as u64);
        driver.set_bits_range(self.io, self.meta.mft_mirr_lcn, mirr_clusters, true)?;

        // Logfile clusters typically already inside [0, first_data_unit) depending on layout.

        driver.flush(self.io)?;
        Ok(())
    }

    // -------------------------
    // Record writers (prefer NtfsMftRecord::new_*)
    // -------------------------

    fn write_mft_record_mft(&mut self, allocator: &mut NtfsAllocator<'a>) -> FsFormatterResult<()> {
        // $MFT data runs
        let mft_clusters = self.meta.initial_mft_clusters();
        let mft_handle = crate::allocator::NtfsHandle::from_range(self.meta.mft_lcn, mft_clusters);
        let mft_dataruns = Self::encode_runs_to_dataruns(&mft_handle.runs);

        // Allocate $MFT::$BITMAP (non-resident, separate runlist)
        let bitmap_size = self.meta.reserved_mft_records / 8;
        let bitmap_clusters = bitmap_size.div_ceil(self.meta.bytes_per_cluster as u64);
        let bitmap_handle = allocator.allocate_contiguous(self.io, bitmap_clusters as usize)?;

        // Initialize $MFT::$BITMAP content (records IN_USE)
        let mut bitmap_data =
            vec![0u8; (bitmap_clusters * self.meta.bytes_per_cluster as u64) as usize];

        // Mark records we actually create as in-use:
        // 0..11 always, plus $Extend children we create.
        let mut mark_in_use = |rec: u64| {
            let i = rec as usize;
            bitmap_data[i / 8] |= 1u8 << (i % 8);
        };

        for rec in 0u64..=MFT_RECORD_EXTEND {
            mark_in_use(rec);
        }
        mark_in_use(MFT_RECORD_QUOTA);
        mark_in_use(MFT_RECORD_OBJID);
        mark_in_use(MFT_RECORD_REPARSE);

        self.io.write_at(
            self.meta.lcn_to_offset(bitmap_handle.start_lcn),
            &bitmap_data,
        )?;

        let bitmap_dataruns = Self::encode_runs_to_dataruns(&bitmap_handle.runs);

        // Build with canonical builder
        let record = NtfsMftRecord::new_mft(
            self.meta,
            mft_dataruns,
            bitmap_dataruns,
            SECURITY_ID_EVERYONE,
        );

        let raw = record.to_raw_buffer(self.meta)?;
        mft::write_record(self.io, self.meta, MFT_RECORD_MFT, &raw).map_err(FsFormatterError::from)
    }

    fn write_mft_record_mftmirr(&mut self) -> FsFormatterResult<()> {
        let mirr_clusters =
            (4 * self.meta.mft_record_size as u64).div_ceil(self.meta.bytes_per_cluster as u64);
        let handle =
            crate::allocator::NtfsHandle::from_range(self.meta.mft_mirr_lcn, mirr_clusters);
        let dataruns = Self::encode_runs_to_dataruns(&handle.runs);

        let record = NtfsMftRecord::new_mftmirr(self.meta, dataruns, SECURITY_ID_EVERYONE);

        let raw = record.to_raw_buffer(self.meta)?;
        mft::write_record(self.io, self.meta, MFT_RECORD_MFTMIRR, &raw)
            .map_err(FsFormatterError::from)
    }

    fn write_mft_record_logfile(&mut self) -> FsFormatterResult<()> {
        let log_size = (2 * 1024 * 1024).min(self.meta.volume_size_bytes / 10);
        let clusters = log_size.div_ceil(self.meta.bytes_per_cluster as u64);

        // Your layout reserves $LogFile at LCN=3 (keep if consistent with boot/system area).
        let handle = crate::allocator::NtfsHandle::from_range(3, clusters);

        // Zero-init content
        let pattern = vec![0u8; self.meta.bytes_per_cluster as usize];
        let offset = self.meta.lcn_to_offset(handle.start_lcn);
        for i in 0..clusters {
            self.io
                .write_at(offset + (i * self.meta.bytes_per_cluster as u64), &pattern)?;
        }

        // IMPORTANT: $LogFile must have non-resident $DATA, not data_empty().
        let dataruns = Self::encode_runs_to_dataruns(&handle.runs);
        let mut record =
            NtfsMftRecord::new_logfile(self.meta, dataruns, log_size, SECURITY_ID_EVERYONE);

        record.add_attribute(NtfsAttribute::non_resident(
            AttributeType::Data,
            "",
            self.meta,
            &handle.runs,
            log_size,
        ));

        let raw = record.to_raw_buffer(self.meta)?;
        mft::write_record(self.io, self.meta, MFT_RECORD_LOGFILE, &raw)
            .map_err(FsFormatterError::from)
    }

    fn write_mft_record_volume(&mut self) -> FsFormatterResult<()> {
        let record = NtfsMftRecord::new_volume(self.meta, SECURITY_ID_EVERYONE);
        let raw = record.to_raw_buffer(self.meta)?;
        mft::write_record(self.io, self.meta, MFT_RECORD_VOLUME, &raw)
            .map_err(FsFormatterError::from)
    }

    fn write_mft_record_attrdef(
        &mut self,
        allocator: &mut NtfsAllocator<'a>,
    ) -> FsFormatterResult<()> {
        let content = attrdef::build_standard_attr_defs();
        let size = content.len() as u64;

        // Keep resident if small, else allocate and make non-resident.
        if size < 600 {
            let record = NtfsMftRecord::new_attrdef(content.clone(), SECURITY_ID_EVERYONE);
            let raw = record.to_raw_buffer(self.meta)?;
            return mft::write_record(self.io, self.meta, MFT_RECORD_ATTRDEF, &raw)
                .map_err(FsFormatterError::from);
        }

        let clusters = size.div_ceil(self.meta.bytes_per_cluster as u64);
        let handle = allocator.allocate_contiguous(self.io, clusters as usize)?;

        let mut content_vec = content.to_vec();
        let mut stream = MemRimIO::new(&mut content_vec);
        crate::core::utils::stream_copy::write_stream_to_run_list(
            self.io,
            self.meta,
            &mut stream,
            &handle.runs,
            size,
        )?;

        // Build record manually but using NtfsAttribute helpers (still consistent).
        // If you want this 100% in NtfsMftRecord::new_attrdef, add an overload that takes RunList.
        let mut record = NtfsMftRecord::new(MFT_RECORD_ATTRDEF as u32, false, true);
        record.add_attribute(NtfsAttribute::standard_info(
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
            SECURITY_ID_EVERYONE,
        ));
        record.add_attribute(NtfsAttribute::file_name(
            Self::root_ref(),
            "$AttrDef",
            size,
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
            NtfsFileNameNamespace::Win32AndDos,
        ));
        record.add_attribute(NtfsAttribute::non_resident(
            AttributeType::Data,
            "",
            self.meta,
            &handle.runs,
            size,
        ));

        let raw = record.to_raw_buffer(self.meta)?;
        mft::write_record(self.io, self.meta, MFT_RECORD_ATTRDEF, &raw)
            .map_err(FsFormatterError::from)
    }

    fn write_mft_record_root(
        &mut self,
        allocator: &mut NtfsAllocator<'a>,
        timestamp: u64,
    ) -> FsFormatterResult<()> {
        let mut entries = Self::build_root_entries(self.meta, timestamp);

        let upcase = UpcaseHandle::from_flavor(&self.meta.upcase_flavor);
        entries.sort_by(|a, b| crate::utils::compare_names_upcase(&a.name, &b.name, &upcase));

        let entries_len: usize = entries.iter().map(|e| e.len()).sum();
        let resident_max = (self.meta.mft_record_size as usize).saturating_sub(400);

        if entries_len < resident_max {
            let mut buf = Vec::new();
            for e in &entries {
                let len = e.len();
                let old = buf.len();
                buf.resize(old + len, 0);
                let mut io = rimio::prelude::MemRimIO::new(&mut buf[old..]);
                e.write_to_io(&mut io, 0).ok();
            }
            let last = crate::types::IndexEntryHeader::new(0, 0, true);
            buf.extend_from_slice(zerocopy::IntoBytes::as_bytes(&last));

            // Root's parent is itself on fresh NTFS (practical/compatible).
            let record = NtfsMftRecord::new_dir(
                MFT_RECORD_ROOT as u32,
                Self::root_ref(),
                ".",
                buf,
                false,
                NtfsFileAttributes::HIDDEN
                    | NtfsFileAttributes::SYSTEM
                    | NtfsFileAttributes::DIRECTORY,
                self.meta,
                SECURITY_ID_EVERYONE,
            );

            let raw = record.to_raw_buffer(self.meta)?;
            mft::write_record(self.io, self.meta, MFT_RECORD_ROOT, &raw)
                .map_err(FsFormatterError::from)
        } else {
            use crate::builder::index_layout::IndexTreeBuilder;

            let layout = IndexTreeBuilder::build(self.meta, entries)?;

            let mut record = NtfsMftRecord::new_dir(
                MFT_RECORD_ROOT as u32,
                Self::root_ref(),
                ".",
                layout.root_entries,
                true,
                NtfsFileAttributes::HIDDEN
                    | NtfsFileAttributes::SYSTEM
                    | NtfsFileAttributes::DIRECTORY,
                self.meta,
                SECURITY_ID_EVERYONE,
            );

            let handle = allocator.allocate_contiguous(self.io, layout.total_clusters as usize)?;

            // Write INDEX_ALLOCATION blocks
            let mut phys_runs = rimio::run::RunList::new();
            for run in handle.runs.iter() {
                phys_runs.push(rimio::run::Run::new(
                    self.meta.lcn_to_offset(run.start),
                    run.length * self.meta.bytes_per_cluster as u64,
                ));
            }

            let mut mapped = rimio::run::MappedRimIO::new(self.io, &phys_runs, 1);
            let mut off = 0u64;
            for block in layout.allocation_blocks.iter() {
                mapped.write_at(off, block)?;
                off += self.meta.index_record_size as u64;
            }

            record.add_attribute(NtfsAttribute::non_resident(
                AttributeType::IndexAllocation,
                "$I30",
                self.meta,
                &handle.runs,
                handle.cluster_count() * self.meta.bytes_per_cluster as u64,
            ));
            record.add_attribute(NtfsAttribute::bitmap_named("$I30", layout.bitmap));

            let raw = record.to_raw_buffer(self.meta)?;
            mft::write_record(self.io, self.meta, MFT_RECORD_ROOT, &raw)
                .map_err(FsFormatterError::from)
        }
    }

    fn write_mft_record_bitmap(&mut self) -> FsFormatterResult<()> {
        let bitmap_size = self.meta.total_clusters.div_ceil(8);
        let clusters = bitmap_size.div_ceil(self.meta.bytes_per_cluster as u64);

        let handle = crate::allocator::NtfsHandle::from_range(self.meta.bitmap_lcn, clusters);

        let dataruns = Self::encode_runs_to_dataruns(&handle.runs);

        let record = NtfsMftRecord::new_bitmap_file(self.meta, dataruns, SECURITY_ID_EVERYONE);

        let raw = record.to_raw_buffer(self.meta)?;
        mft::write_record(self.io, self.meta, MFT_RECORD_BITMAP, &raw)
            .map_err(FsFormatterError::from)
    }

    fn write_mft_record_boot(&mut self) -> FsFormatterResult<()> {
        let boot_size = 16 * self.meta.bytes_per_sector as u64;
        let clusters = boot_size.div_ceil(self.meta.bytes_per_cluster as u64);

        // LCN 0
        let handle = crate::allocator::NtfsHandle::from_range(0, clusters);
        let dataruns = Self::encode_runs_to_dataruns(&handle.runs);

        let record = NtfsMftRecord::new_boot_file(self.meta, dataruns, SECURITY_ID_EVERYONE);

        let raw = record.to_raw_buffer(self.meta)?;
        mft::write_record(self.io, self.meta, MFT_RECORD_BOOT, &raw).map_err(FsFormatterError::from)
    }

    fn write_mft_record_badclus(&mut self) -> FsFormatterResult<()> {
        // Keep your existing API (some versions take meta, some don’t).
        let record = NtfsMftRecord::new_badclus(SECURITY_ID_EVERYONE);
        let raw = record.to_raw_buffer(self.meta)?;
        mft::write_record(self.io, self.meta, MFT_RECORD_BADCLUS, &raw)
            .map_err(FsFormatterError::from)
    }

    fn write_mft_record_secure(
        &mut self,
        allocator: &mut NtfsAllocator<'a>,
    ) -> FsFormatterResult<()> {
        let content = crate::system::secure::build_secure_content();
        let sds_len = content.sds.len() as u64;

        let clusters = sds_len.div_ceil(self.meta.bytes_per_cluster as u64);
        let handle = allocator.allocate_contiguous(self.io, clusters as usize)?;

        // Zero-init
        let zero = vec![0u8; self.meta.bytes_per_cluster as usize];
        let sds_offset = self.meta.lcn_to_offset(handle.start_lcn);
        for i in 0..clusters {
            self.io
                .write_at(sds_offset + (i * self.meta.bytes_per_cluster as u64), &zero)?;
        }

        // Write $SDS content
        let mut binding = content.sds.clone();
        let mut stream = MemRimIO::new(&mut binding);
        crate::core::utils::stream_copy::write_stream_to_run_list(
            self.io,
            self.meta,
            &mut stream,
            &handle.runs,
            sds_len,
        )?;

        let allocated_size = clusters * self.meta.bytes_per_cluster as u64;

        let record = NtfsMftRecord::new_secure(
            self.meta,
            &handle.runs,
            allocated_size,
            content.sii_entries,
            content.sdh_entries,
            sds_len,
            SECURITY_ID_EVERYONE,
        );

        let raw = record.to_raw_buffer(self.meta)?;
        mft::write_record(self.io, self.meta, MFT_RECORD_SECURE, &raw)
            .map_err(FsFormatterError::from)
    }

    fn write_mft_record_upcase(&mut self) -> FsFormatterResult<()> {
        let upcase = UpcaseHandle::from_flavor(&self.meta.upcase_flavor);
        let bytes = upcase.as_bytes();
        let size = bytes.len() as u64;

        let clusters = size.div_ceil(self.meta.bytes_per_cluster as u64);

        // Upcase sits after bitmap clusters (your current layout).
        let bitmap_clusters = self
            .meta
            .bitmap_size_bytes
            .div_ceil(self.meta.bytes_per_cluster as u64);
        let upcase_lcn = self.meta.bitmap_lcn + bitmap_clusters;

        let handle = crate::allocator::NtfsHandle::from_range(upcase_lcn, clusters);

        let mut bytes_vec = bytes.to_vec();
        let mut stream = MemRimIO::new(&mut bytes_vec);
        crate::core::utils::stream_copy::write_stream_to_run_list(
            self.io,
            self.meta,
            &mut stream,
            &handle.runs,
            size,
        )?;

        let dataruns = Self::encode_runs_to_dataruns(&handle.runs);

        let record = NtfsMftRecord::new_upcase(self.meta, dataruns, size, SECURITY_ID_EVERYONE);

        let raw = record.to_raw_buffer(self.meta)?;
        mft::write_record(self.io, self.meta, MFT_RECORD_UPCASE, &raw)
            .map_err(FsFormatterError::from)
    }

    fn write_mft_record_extend(&mut self, timestamp: u64) -> FsFormatterResult<()> {
        let record = NtfsMftRecord::new_extend(self.meta, timestamp, SECURITY_ID_EVERYONE);
        let raw = record.to_raw_buffer(self.meta)?;
        mft::write_record(self.io, self.meta, MFT_RECORD_EXTEND, &raw)
            .map_err(FsFormatterError::from)
    }

    // -------------------------
    // $Extend children (minimal, to avoid CHKDSK creating them)
    // -------------------------

    fn write_mft_record_objid(&mut self, timestamp: u64) -> FsFormatterResult<()> {
        self.write_extend_child_file(MFT_RECORD_OBJID, "$ObjId", &[("$O", 0, 0x11)], timestamp)
    }

    fn write_mft_record_reparse(&mut self, timestamp: u64) -> FsFormatterResult<()> {
        self.write_extend_child_file(
            MFT_RECORD_REPARSE,
            "$Reparse",
            &[("$R", 0, 0x11)],
            timestamp,
        )
    }

    fn write_mft_record_quota(&mut self, timestamp: u64) -> FsFormatterResult<()> {
        self.write_extend_child_file(
            MFT_RECORD_QUOTA,
            "$Quota",
            &[("$O", 0, 0x11), ("$Q", 0, 0x10)],
            timestamp,
        )
    }

    fn write_extend_child_file(
        &mut self,
        record_no: u64,
        name: &'static str,
        indices: &[(&'static str, u32, u32)],
        timestamp: u64,
    ) -> FsFormatterResult<()> {
        let extend_ref = Self::extend_ref();

        let mut record = NtfsMftRecord::new(record_no as u32, false, true);

        record.add_attribute(NtfsAttribute::standard_info_custom(
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
            SECURITY_ID_EVERYONE,
            timestamp,
        ));
        record.add_attribute(NtfsAttribute::file_name_custom(
            extend_ref,
            name,
            0,
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
            NtfsFileNameNamespace::Win32AndDos,
            timestamp,
        ));

        record.add_attribute(NtfsAttribute::data_empty());

        // Standard Index End Marker (16 bytes)
        let mut end_marker = vec![0u8; 16];
        end_marker[8] = 0x10; // entry_length = 16
        end_marker[12] = 0x02; // flags = LAST_ENTRY

        let clusters_per_index = if self.meta.index_record_size >= self.meta.bytes_per_cluster {
            (self.meta.index_record_size / self.meta.bytes_per_cluster) as i8
        } else {
            -(self.meta.index_record_size.trailing_zeros() as i8)
        };

        for (idx_name, indexed_attr_type, collation_rule) in indices {
            record.add_attribute(NtfsAttribute::index_root_named(
                idx_name,
                *indexed_attr_type,
                *collation_rule,
                end_marker.clone(), // contient déjà le LAST_ENTRY
                clusters_per_index,
                self.meta.index_record_size,
                false, // has_children = false
            ));
        }

        let raw = record.to_raw_buffer(self.meta)?;
        mft::write_record(self.io, self.meta, record_no, &raw).map_err(FsFormatterError::from)
    }

    fn write_empty_mft_record(&mut self, record_number: u64) -> FsFormatterResult<()> {
        let record = NtfsMftRecord::new(record_number as u32, false, false);
        let raw = record.to_raw_buffer(self.meta)?;
        mft::write_record(self.io, self.meta, record_number, &raw)?;
        Ok(())
    }

    // -------------------------
    // Root index entries
    // -------------------------

    fn build_root_entries(meta: &NtfsMeta, timestamp: u64) -> Vec<NtfsIndexEntry> {
        let root_ref = Self::root_ref();
        let sys = NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM;

        let mft_size = meta.reserved_mft_records * meta.mft_record_size as u64;
        let log_size = (2 * 1024 * 1024).min(meta.volume_size_bytes / 10);
        let attr_def_size = attrdef::build_standard_attr_defs().len() as u64;
        // let bitmap_size = meta.total_clusters.div_ceil(8);
        let boot_size = 16 * meta.bytes_per_sector as u64;
        let upcase_size = UpcaseHandle::from_flavor(&meta.upcase_flavor)
            .as_bytes()
            .len() as u64;

        let items: &[(u64, &str, NtfsFileAttributes, u64)] = &[
            (MFT_RECORD_MFT, "$MFT", sys, mft_size),
            (
                MFT_RECORD_MFTMIRR,
                "$MFTMirr",
                sys,
                4 * meta.mft_record_size as u64,
            ),
            (MFT_RECORD_LOGFILE, "$LogFile", sys, log_size),
            (
                MFT_RECORD_VOLUME,
                "$Volume",
                sys | NtfsFileAttributes::READ_ONLY,
                0,
            ),
            (MFT_RECORD_ATTRDEF, "$AttrDef", sys, attr_def_size),
            (MFT_RECORD_BITMAP, "$Bitmap", sys, 0),
            (MFT_RECORD_BOOT, "$Boot", sys, boot_size),
            (MFT_RECORD_BADCLUS, "$BadClus", sys, 0),
            (MFT_RECORD_SECURE, "$Secure", sys, 0),
            (MFT_RECORD_UPCASE, "$UpCase", sys, upcase_size),
            (
                MFT_RECORD_EXTEND,
                "$Extend",
                sys | NtfsFileAttributes::DIRECTORY,
                0,
            ),
        ];

        items
            .iter()
            .map(|&(rec, name, attrs, size)| {
                NtfsIndexEntry::new(
                    sys_ref(rec),
                    root_ref,
                    name.encode_utf16().collect(),
                    attrs,
                    IndexEntryFlags::empty(),
                    None,
                )
                .with_timestamps(timestamp)
                .with_namespace(NtfsFileNameNamespace::Win32AndDos)
                .with_sizes(
                    size,
                    size.div_ceil(meta.bytes_per_cluster as u64) * meta.bytes_per_cluster as u64,
                )
            })
            .collect()
    }
}

impl<'a, IO: RimIO + ?Sized> FsFormatter for NtfsFormatter<'a, IO> {
    fn format(&mut self, full_format: bool) -> FsFormatterResult<()> {
        self.format(full_format)
    }
}

#[cfg(test)]
mod tests {
    // Add integration tests at the workspace level (mount + chkdsk).
}
