// SPDX-License-Identifier: MIT

//! NTFS filesystem consistency and metadata integrity checker.

use crate::constant::*;
use crate::core::bitmap::BitmapDriver;
use crate::core::checker::{
    Finding, FsCheckerError, FsCheckerResult, VerifierOptionsLike, VerifyPhases, VerifyReport,
};
use crate::core::traits::FsChecker;
use crate::meta::NtfsMeta;
use crate::mft;
use crate::types::NtfsBootSector;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::format;
use rimio::RimIO;
use rimio::RimReadStructExt;
use zerocopy::FromBytes;
use zerocopy::IntoBytes;

#[derive(Debug, Clone)]
pub struct NtfsCheckerOptions {
    phases: VerifyPhases,
    fail_fast: bool,
}

impl Default for NtfsCheckerOptions {
    fn default() -> Self {
        Self {
            phases: VerifyPhases::ALL,
            fail_fast: false,
        }
    }
}

impl VerifierOptionsLike for NtfsCheckerOptions {
    fn phases(&self) -> VerifyPhases {
        self.phases.clone()
    }

    fn fail_fast(&self) -> bool {
        self.fail_fast
    }
}

/// NTFS filesystem checker
pub struct NtfsChecker<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a NtfsMeta,
}

impl<'a, IO: RimIO + ?Sized> NtfsChecker<'a, IO> {
    /// Create a new NTFS checker
    pub fn new(io: &'a mut IO, meta: &'a NtfsMeta) -> Self {
        Self { io, meta }
    }

    /// Check structural index integrity ($INDEX_ROOT, $INDEX_ALLOCATION, $BITMAP, child VCNs) of a directory
    pub fn check_directory_index(
        &mut self,
        mft_num: u64,
        dir_name: &str,
        rep: &mut VerifyReport,
    ) -> FsCheckerResult<()> {
        let mut resolver = crate::resolver::NtfsResolver::new(self.io, self.meta);
        index::check_dir_index_with_resolver(self.meta, &mut resolver, mft_num, dir_name, rep)
    }
}

pub mod index;
pub mod system;

#[cfg(test)]
mod tests;

impl<'a, IO: RimIO + ?Sized> FsChecker for NtfsChecker<'a, IO> {
    type Options = NtfsCheckerOptions;

    fn check_boot(&mut self, _opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        let boot: NtfsBootSector = self.io.read_struct(0).map_err(FsCheckerError::IO)?;

        let mut primary_valid = true;
        let oem_id = boot.oem_id;
        if &oem_id != b"NTFS    " {
            rep.push(Finding::err(
                "BOOT.OEM",
                format!("Invalid OEM ID: {:?}", oem_id),
            ));
            primary_valid = false;
        } else {
            rep.push(Finding::info("BOOT.OEM", "NTFS OEM ID OK"));
        }

        let end_marker = boot.end_marker.get();
        if end_marker != 0xAA55 {
            rep.push(Finding::err(
                "BOOT.SIG",
                format!("Invalid boot signature: 0x{:04X}", end_marker),
            ));
            primary_valid = false;
        } else {
            rep.push(Finding::info("BOOT.SIG", "Boot signature 0xAA55 OK"));
        }

        let bps = boot.bytes_per_sector.get();
        if bps != 512 && bps != 1024 && bps != 2048 && bps != 4096 {
            rep.push(Finding::warn(
                "BOOT.BPS",
                format!("Unusual sector size: {bps} bytes"),
            ));
        } else {
            rep.push(Finding::info(
                "BOOT.BPS",
                format!("Sector size: {bps} bytes"),
            ));
        }

        let spc = boot.sectors_per_cluster;
        if !spc.is_power_of_two() {
            rep.push(Finding::err(
                "BOOT.SPC",
                format!("Sectors per cluster {spc} is not a power of 2"),
            ));
            primary_valid = false;
        } else {
            rep.push(Finding::info(
                "BOOT.SPC",
                format!("Sectors per cluster: {spc}"),
            ));
        }

        if primary_valid {
            rep.push(Finding::info(
                "BOOT.PRIMARY",
                "Primary NTFS boot sector valid",
            ));
        }

        let backup_offset = self.meta.backup_boot_sector_offset();
        match self.io.read_struct::<NtfsBootSector>(backup_offset) {
            Ok(backup_boot) => {
                let mut backup_valid = true;
                if backup_boot.oem_id != NTFS_BOOT_SIGNATURE {
                    rep.push(Finding::err(
                        "BOOT.BACKUP",
                        format!(
                            "Alternate boot sector invalid OEM ID: {:?}",
                            backup_boot.oem_id
                        ),
                    ));
                    backup_valid = false;
                }
                let backup_end_marker = backup_boot.end_marker.get();
                if backup_end_marker != 0xAA55 {
                    rep.push(Finding::err(
                        "BOOT.BACKUP",
                        format!(
                            "Alternate invalid boot signature: 0x{:04X}",
                            backup_end_marker
                        ),
                    ));
                    backup_valid = false;
                }

                if backup_valid {
                    rep.push(Finding::info(
                        "BOOT.BACKUP",
                        "Alternate NTFS boot sector valid",
                    ));
                }

                if boot.as_bytes() == backup_boot.as_bytes() {
                    rep.push(Finding::info(
                        "BOOT.MIRROR",
                        "Primary and alternate boot sectors match",
                    ));
                } else {
                    rep.push(Finding::err(
                        "BOOT.MIRROR",
                        "Primary and alternate boot sectors do not match",
                    ));
                }
            }
            Err(e) => {
                rep.push(Finding::err(
                    "BOOT.BACKUP",
                    format!("Failed to read alternate boot sector at offset {backup_offset}: {e}"),
                ));
            }
        }

        Ok(())
    }

    fn check_geometry(
        &mut self,
        _opt: &Self::Options,
        rep: &mut VerifyReport,
    ) -> FsCheckerResult<()> {
        // Validate MFT LCN
        if self.meta.mft_lcn >= self.meta.total_clusters {
            rep.push(Finding::err(
                "GEOM.MFT",
                format!(
                    "MFT LCN {} out of range (total clusters: {})",
                    self.meta.mft_lcn, self.meta.total_clusters
                ),
            ));
        } else {
            rep.push(Finding::info(
                "GEOM.MFT",
                format!("MFT LCN {} OK", self.meta.mft_lcn),
            ));
        }

        // Validate MFT Mirror LCN
        if self.meta.mft_mirr_lcn >= self.meta.total_clusters {
            rep.push(Finding::err(
                "GEOM.MIRR",
                format!("MFTMirr LCN {} out of range", self.meta.mft_mirr_lcn),
            ));
        } else {
            rep.push(Finding::info(
                "GEOM.MIRR",
                format!("MFTMirr LCN {} OK", self.meta.mft_mirr_lcn),
            ));
        }

        // Verify primary MFT system records (0: $MFT, 1: $MFTMirr, 3: $Volume, 6: $Bitmap, 7: $Boot, 10: $UpCase)
        for (rec_num, name) in [
            (MFT_RECORD_MFT, "$MFT"),
            (MFT_RECORD_MFTMIRR, "$MFTMirr"),
            (MFT_RECORD_VOLUME, "$Volume"),
            (MFT_RECORD_BITMAP, "$Bitmap"),
            (MFT_RECORD_BOOT, "$Boot"),
            (MFT_RECORD_UPCASE, "$UpCase"),
        ] {
            let offset = self.meta.mft_record_offset(rec_num);
            let record_size = self.meta.mft_record_size as usize;
            let sector_size = self.meta.bytes_per_sector as usize;

            let mut raw = vec![0u8; record_size];
            if let Err(e) = self.io.read_at(offset, &mut raw) {
                rep.push(Finding::err(
                    "MFT.REC",
                    format!("Failed to read raw {name} (record {rec_num}): {e}"),
                ));
                continue;
            }

            if raw[0..4] != NTFS_FILE_SIGNATURE {
                rep.push(Finding::err(
                    "MFT.SIG",
                    format!(
                        "Record {rec_num} ({name}) invalid signature: {:?}",
                        &raw[0..4]
                    ),
                ));
                continue;
            }

            let (header, _) = crate::types::MftRecordHeader::ref_from_prefix(&raw)
                .map_err(|_| FsCheckerError::Invalid("Truncated MFT header"))?;
            let usa_ofs = header.usa_offset.get() as usize;
            let usa_cnt = header.usa_count.get() as usize;
            let expected_usa_cnt = (record_size / sector_size) + 1;

            if usa_cnt != expected_usa_cnt {
                rep.push(Finding::err(
                    "MFT.USA",
                    format!(
                        "Record {rec_num} ({name}) invalid usa_count {usa_cnt} (expected {expected_usa_cnt})"
                    ),
                ));
                continue;
            }

            if usa_ofs < 48 || usa_ofs + usa_cnt * 2 > record_size {
                rep.push(Finding::err(
                    "MFT.USA",
                    format!("Record {rec_num} ({name}) invalid usa_offset {usa_ofs}"),
                ));
                continue;
            }

            let usn = u16::from_le_bytes([raw[usa_ofs], raw[usa_ofs + 1]]);
            let mut trailer_ok = true;
            for i in 1..usa_cnt {
                let sector_end = i * sector_size - 2;
                let trailer = u16::from_le_bytes([raw[sector_end], raw[sector_end + 1]]);
                if trailer != usn {
                    rep.push(Finding::err(
                        "MFT.TRAILER",
                        format!(
                            "Record {rec_num} ({name}) sector {i} trailer 0x{trailer:04X} != USN 0x{usn:04X}"
                        ),
                    ));
                    trailer_ok = false;
                    break;
                }
            }
            if !trailer_ok {
                continue;
            }

            if !crate::utils::decode_usa_fixup(&mut raw, sector_size) {
                rep.push(Finding::err(
                    "MFT.USA",
                    format!("Record {rec_num} ({name}) decode_usa_fixup failed"),
                ));
                continue;
            }

            rep.push(Finding::info(
                "MFT.REC",
                format!("System record {rec_num} ({name}) readable"),
            ));
        }

        // Validate $UpCase stream
        let mut resolver = crate::resolver::NtfsResolver::new(self.io, self.meta);
        match resolver.read_file_stream(MFT_RECORD_UPCASE, None) {
            Ok(upcase_bytes) => {
                if upcase_bytes.len() == 131072 {
                    rep.push(Finding::info(
                        "UPCASE.SIZE",
                        "$UpCase size OK (131072 bytes)",
                    ));
                    if crate::upcase::UpcaseHandle::from_le_bytes(&upcase_bytes).is_ok() {
                        rep.push(Finding::info(
                            "UPCASE.DATA",
                            "$UpCase data stream structurally valid",
                        ));
                    } else {
                        rep.push(Finding::err(
                            "UPCASE.DATA",
                            "Invalid Unicode table in $UpCase data stream",
                        ));
                    }
                } else {
                    rep.push(Finding::err(
                        "UPCASE.SIZE",
                        format!(
                            "Invalid $UpCase size {} (expected 131072 bytes)",
                            upcase_bytes.len()
                        ),
                    ));
                }
            }
            Err(e) => {
                rep.push(Finding::err(
                    "UPCASE.DATA",
                    format!("Failed to read $UpCase data stream: {e}"),
                ));
            }
        }

        self.check_windows_system_invariants(rep)?;
        Ok(())
    }

    fn check_root(&mut self, _opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        match mft::read_record(self.io, self.meta, MFT_RECORD_ROOT) {
            Ok(_) => {
                rep.push(Finding::info(
                    "ROOT.REC",
                    "Root directory record (5) readable",
                ));
                self.check_directory_index(MFT_RECORD_ROOT, "$Root", rep)?;
            }
            Err(e) => {
                rep.push(Finding::err(
                    "ROOT.REC",
                    format!("Failed to read root directory record: {e}"),
                ));
            }
        }
        Ok(())
    }

    fn check_chain(&mut self, _opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        let min_expected_bytes = self.meta.total_clusters.div_ceil(8);
        if self.meta.bitmap_size_bytes < min_expected_bytes {
            rep.push(Finding::warn(
                "CHAIN.BITMAP",
                format!(
                    "$Bitmap size {} < total cluster bytes {}",
                    self.meta.bitmap_size_bytes, min_expected_bytes
                ),
            ));
        } else {
            rep.push(Finding::info(
                "CHAIN.BITMAP",
                format!("$Bitmap size OK ({} bytes)", self.meta.bitmap_size_bytes),
            ));
        }

        // 1. Scan records 0..16 to collect non-resident runs
        let mut runs_to_check = alloc::vec::Vec::new();
        {
            let mut resolver = crate::resolver::NtfsResolver::new(self.io, self.meta);
            for rec_num in 0..16 {
                if let Ok(rec) = resolver.read_mft_record(rec_num) {
                    let view = match crate::view::mft_view::MftRecordView::new(&rec) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    for attr in view.attrs().flatten() {
                        if let Ok(crate::view::attr_view::AttrView::NonResident {
                            runlist, ..
                        }) = attr.as_view()
                        {
                            for run in runlist.iter() {
                                if let Some(lcn) = run.lcn {
                                    runs_to_check.push((rec_num, lcn, run.len));
                                }
                            }
                        }
                    }
                }
            }
        }

        // 2. Verify non-resident runs are marked in $Bitmap using BitmapDriver
        let mut driver = BitmapDriver::new(self.meta);
        let mut checked_runs = 0usize;
        for (rec_num, lcn, len) in runs_to_check {
            checked_runs += 1;
            for i in 0..len {
                let cluster = lcn + i;
                let is_set = driver.get_bit_ro(self.io, cluster).unwrap_or(false);
                if !is_set {
                    rep.push(Finding::err(
                        "CHAIN.ALLOC",
                        format!(
                            "Record {rec_num} uses cluster {cluster} but marked free in $Bitmap"
                        ),
                    ));
                }
            }
        }

        rep.push(Finding::info(
            "CHAIN.OK",
            format!("Validated {checked_runs} non-resident cluster runs against $Bitmap"),
        ));
        Ok(())
    }

    fn check_cross_reference(
        &mut self,
        _opt: &Self::Options,
        rep: &mut VerifyReport,
    ) -> FsCheckerResult<()> {
        let mut resolver = crate::resolver::NtfsResolver::new(self.io, self.meta);
        let mut queue = vec![MFT_RECORD_ROOT];
        let mut visited = alloc::vec::Vec::new();
        let mut total_entries = 0usize;

        while let Some(dir_rec) = queue.pop() {
            if visited.contains(&dir_rec) {
                continue;
            }
            visited.push(dir_rec);

            if dir_rec != MFT_RECORD_ROOT {
                let _ = index::check_dir_index_with_resolver(
                    self.meta,
                    &mut resolver,
                    dir_rec,
                    &format!("Directory record {dir_rec}"),
                    rep,
                );
            }

            match resolver.read_directory_entries(dir_rec) {
                Ok(entries) => {
                    for (_name, child_rec, attr) in entries {
                        total_entries += 1;
                        if attr.is_dir()
                            && child_rec != MFT_RECORD_ROOT
                            && !visited.contains(&child_rec)
                        {
                            queue.push(child_rec);
                        }
                    }
                }
                Err(e) => {
                    rep.push(Finding::warn(
                        "CROSSREF.DIR",
                        format!("Failed to read directory record {dir_rec}: {e}"),
                    ));
                }
            }
        }

        rep.push(Finding::info(
            "CROSSREF.OK",
            format!(
                "Directory tree verified: {} directories visited, {} entries checked",
                visited.len(),
                total_entries
            ),
        ));

        Ok(())
    }
}
