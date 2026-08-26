#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::format;
use rimio::RimIO;
use zerocopy::FromBytes;

use crate::constant::*;
use crate::core::checker::{
    Finding, FsCheckerError, FsCheckerResult, VerifierOptionsLike, VerifyPhases, VerifyReport,
};
use crate::core::traits::FsChecker;
use crate::meta::NtfsMeta;
use crate::mft;
use crate::types::NtfsBootSector;

/// NTFS verifier options
#[derive(Debug, Clone)]
pub struct NtfsVerifierOptions {
    phases: VerifyPhases,
    fail_fast: bool,
}

impl Default for NtfsVerifierOptions {
    fn default() -> Self {
        Self {
            phases: VerifyPhases::ALL,
            fail_fast: false,
        }
    }
}

impl VerifierOptionsLike for NtfsVerifierOptions {
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
}

impl<'a, IO: RimIO + ?Sized> FsChecker for NtfsChecker<'a, IO> {
    type Options = NtfsVerifierOptions;

    fn check_boot(&mut self, _opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        let mut boot_buf = [0u8; 512];
        self.io
            .read_at(0, &mut boot_buf)
            .map_err(FsCheckerError::IO)?;

        let boot = NtfsBootSector::read_from_bytes(&boot_buf)
            .map_err(|_| FsCheckerError::Invalid("Failed to read NTFS boot sector"))?;

        let oem_id = boot.oem_id;
        if &oem_id != b"NTFS    " {
            rep.push(Finding::err(
                "BOOT.OEM",
                format!("Invalid OEM ID: {:?}", oem_id),
            ));
        } else {
            rep.push(Finding::info("BOOT.OEM", "NTFS OEM ID OK"));
        }

        let end_marker = boot.end_marker;
        if end_marker != 0xAA55 {
            rep.push(Finding::err(
                "BOOT.SIG",
                format!("Invalid boot signature: 0x{:04X}", end_marker),
            ));
        } else {
            rep.push(Finding::info("BOOT.SIG", "Boot signature 0xAA55 OK"));
        }

        let bps = boot.bytes_per_sector;
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
        } else {
            rep.push(Finding::info(
                "BOOT.SPC",
                format!("Sectors per cluster: {spc}"),
            ));
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
            match mft::read_record(self.io, self.meta, rec_num) {
                Ok(_) => {
                    rep.push(Finding::info(
                        "MFT.REC",
                        format!("System record {rec_num} ({name}) readable"),
                    ));
                }
                Err(e) => {
                    rep.push(Finding::err(
                        "MFT.REC",
                        format!("Failed to read {name} (record {rec_num}): {e}"),
                    ));
                }
            }
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
                    if crate::system::upcase::UpcaseHandle::from_le_bytes(&upcase_bytes).is_ok() {
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

        Ok(())
    }

    fn check_root(&mut self, _opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        match mft::read_record(self.io, self.meta, MFT_RECORD_ROOT) {
            Ok(_) => {
                rep.push(Finding::info("ROOT.REC", "Root directory record (5) OK"));
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
        let mut resolver = crate::resolver::NtfsResolver::new(self.io, self.meta);

        // 1. Read $Bitmap content (record 6)
        let bitmap_bytes = match resolver.read_file_stream(MFT_RECORD_BITMAP, None) {
            Ok(b) => b,
            Err(e) => {
                rep.push(Finding::err(
                    "CHAIN.BITMAP",
                    format!("Failed to read $Bitmap data stream: {e}"),
                ));
                return Ok(());
            }
        };

        let min_expected_bytes = (self.meta.total_clusters).div_ceil(8) as usize;
        if bitmap_bytes.len() < min_expected_bytes {
            rep.push(Finding::warn(
                "CHAIN.BITMAP",
                format!(
                    "$Bitmap size {} < total cluster bytes {}",
                    bitmap_bytes.len(),
                    min_expected_bytes
                ),
            ));
        } else {
            rep.push(Finding::info(
                "CHAIN.BITMAP",
                format!("$Bitmap data stream size OK ({} bytes)", bitmap_bytes.len()),
            ));
        }

        // 2. Scan records 0..16 to verify non-resident runs are marked in $Bitmap
        let mut checked_runs = 0usize;
        for rec_num in 0..16 {
            if let Ok(rec) = resolver.read_mft_record(rec_num) {
                let view = match crate::view::mft_view::MftRecordView::new(&rec) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                for attr in view.attrs().flatten() {
                    if let Ok(crate::view::attr_view::AttrView::NonResident { runlist, .. }) =
                        attr.as_view()
                    {
                        for run in runlist.iter() {
                            if let Some(lcn) = run.lcn {
                                checked_runs += 1;
                                for i in 0..run.len {
                                    let cluster = lcn + i;
                                    let byte_idx = (cluster / 8) as usize;
                                    let bit_idx = (cluster % 8) as usize;
                                    if byte_idx < bitmap_bytes.len() {
                                        let is_set = (bitmap_bytes[byte_idx] & (1 << bit_idx)) != 0;
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
                            }
                        }
                    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formatter::NtfsFormatter;
    use rimio::prelude::MemRimIO;

    #[test]
    fn test_ntfs_checker_basic() {
        let meta = NtfsMeta::new(5 * 1024 * 1024, Some("NTFS_TEST")).unwrap();
        let mut buffer = vec![0u8; 5 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut buffer);

        NtfsFormatter::new(&mut io, &meta).format(true).unwrap();

        let mut checker = NtfsChecker::new(&mut io, &meta);
        let report = checker.check_all().unwrap();
        assert!(
            !report.has_error(),
            "Checker report had errors: {:?}",
            report.findings
        );
        assert!(report.findings.iter().any(|f| f.code == "BOOT.OEM"));
        assert!(report.findings.iter().any(|f| f.code == "GEOM.MFT"));
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.code == "MFT.REC" && f.msg.contains("$UpCase"))
        );
        assert!(report.findings.iter().any(|f| f.code == "UPCASE.SIZE"));
        assert!(report.findings.iter().any(|f| f.code == "UPCASE.DATA"));
        assert!(report.findings.iter().any(|f| f.code == "ROOT.REC"));
        assert!(report.findings.iter().any(|f| f.code == "CHAIN.OK"));
        assert!(report.findings.iter().any(|f| f.code == "CROSSREF.OK"));
    }

    #[test]
    fn test_ntfs_upcase_record_10_regression() {
        let meta = NtfsMeta::new(5 * 1024 * 1024, Some("NTFS_UPCASE")).unwrap();
        let mut buffer = vec![0u8; 5 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut buffer);

        NtfsFormatter::new(&mut io, &meta).format(true).unwrap();

        // 1. Read MFT record 10 ($UpCase)
        let mut resolver = crate::resolver::NtfsResolver::new(&mut io, &meta);
        let rec = resolver
            .read_mft_record(MFT_RECORD_UPCASE)
            .expect("$UpCase record readable");

        let view = crate::view::mft_view::MftRecordView::new(&rec).expect("Valid MFT view");

        // 2. Find unnamed $DATA attribute
        let data_attr = view
            .find_named(crate::constant::ATTR_DATA, None)
            .expect("Valid attribute parse")
            .expect("Unnamed $DATA exists");

        // 3. Verify non-resident and size properties
        assert!(!data_attr.is_resident(), "non-resident == true");
        let attr_view = data_attr.as_view().expect("Valid attr view");
        match attr_view {
            crate::view::attr_view::AttrView::NonResident {
                allocated_size,
                data_size,
                initialized_size,
                runlist,
                ..
            } => {
                assert_eq!(data_size, 131072, "data_size == 131072");
                assert_eq!(initialized_size, 131072, "initialized_size == 131072");
                assert!(allocated_size >= 131072, "allocated_size >= 131072");
                assert!(runlist.iter().count() > 0, "runlist resolves correctly");
            }
            _ => panic!("Expected NonResident attribute"),
        }

        // 4. Verify payload read via resolver
        let upcase_payload = resolver
            .read_file_stream(MFT_RECORD_UPCASE, None)
            .expect("Payload stream readable");
        assert_eq!(upcase_payload.len(), 131072, "payload length == 131072");

        // 5. Verify $FILE_NAME data_size
        let fn_attr = view
            .find_named(crate::constant::ATTR_FILE_NAME, None)
            .expect("Valid attr parse")
            .expect("$FILE_NAME exists");
        let fn_val = resolver.get_resident_attribute_content(fn_attr).unwrap();
        let fn_struct = *crate::types::FileNameAttribute::ref_from_prefix(fn_val)
            .unwrap()
            .0;
        let fn_data_size = fn_struct.data_size;
        let fn_allocated_size = fn_struct.allocated_size;
        assert_eq!(fn_data_size, 131072, "$FILE_NAME data_size == 131072");
        assert!(
            fn_allocated_size >= 131072,
            "$FILE_NAME allocated_size >= 131072"
        );
    }
}
