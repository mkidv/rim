// SPDX-License-Identifier: MIT
#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::{vec, vec::Vec};

use crate::meta::ZipMeta;
use crate::types::*;
use crc32fast::Hasher;
use rimfs_core::checker::{Finding, FsChecker, FsCheckerResult, VerifierOptionsLike, VerifyReport};
use rimio::RimIO;

/// Configuration options for ZIP archive verification.
#[derive(Debug, Clone)]
pub struct ZipCheckerOptions {
    pub verify_checksums: bool,
}
impl VerifierOptionsLike for ZipCheckerOptions {}

impl Default for ZipCheckerOptions {
    fn default() -> Self {
        Self {
            verify_checksums: true,
        }
    }
}

/// Validates ZIP archive headers, central directory records, and payload CRC32 checksums.
pub struct ZipChecker<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    _meta: &'a ZipMeta,
    pub options: ZipCheckerOptions,
}

impl<'a, IO: RimIO + ?Sized> ZipChecker<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ZipMeta) -> Self {
        Self {
            io,
            _meta: meta,
            options: ZipCheckerOptions::default(),
        }
    }
}

impl<'a, IO: RimIO + ?Sized> FsChecker for ZipChecker<'a, IO> {
    type Options = ZipCheckerOptions;

    fn check_root(&mut self, _opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        let _ = read_entries(self.io, rep)?;
        Ok(())
    }

    fn check_content(
        &mut self,
        opt: &Self::Options,
        rep: &mut VerifyReport,
    ) -> FsCheckerResult<()> {
        if !opt.verify_checksums {
            return Ok(());
        }

        let entries = read_entries(self.io, rep)?;
        let mut crc_buf = [0u8; 65536];
        for entry in entries {
            if entry.comp_size == 0 || entry.compression_method != METHOD_STORE {
                continue;
            }

            let mut hasher = Hasher::new();
            let mut remaining = entry.comp_size;
            let mut read_off = entry.data_offset;

            while remaining > 0 {
                let to_read = remaining.min(crc_buf.len() as u64) as usize;
                self.io.read_at(read_off, &mut crc_buf[..to_read])?;
                hasher.update(&crc_buf[..to_read]);
                read_off += to_read as u64;
                remaining -= to_read as u64;
            }

            if hasher.finalize() != entry.expected_crc32 {
                rep.push(Finding::err(
                    "ZIP.CRC",
                    "CRC-32 checksum mismatch for stored entry",
                ));
                return Ok(());
            }
        }
        Ok(())
    }
}

struct ZipEntryCheck {
    compression_method: u16,
    expected_crc32: u32,
    comp_size: u64,
    data_offset: u64,
}

fn read_entries<IO: RimIO + ?Sized>(
    io: &mut IO,
    rep: &mut VerifyReport,
) -> FsCheckerResult<Vec<ZipEntryCheck>> {
    let total_len = io.total_size().unwrap_or(0);
    if total_len < END_OF_CENTRAL_DIR_FIXED_SIZE as u64 {
        rep.push(Finding::err(
            "ZIP.SIZE",
            "ZIP archive is too small to contain an End of Central Directory record",
        ));
        return Ok(Vec::new());
    }

    let eocd_offset = match find_eocd(io, total_len)? {
        Some(pos) => pos,
        None => {
            rep.push(Finding::err(
                "ZIP.EOCD",
                "End of Central Directory (EOCD) signature not found",
            ));
            return Ok(Vec::new());
        }
    };

    let mut eocd_buf = [0u8; END_OF_CENTRAL_DIR_FIXED_SIZE];
    io.read_at(eocd_offset, &mut eocd_buf)?;

    let total_entries = u16::from_le_bytes([eocd_buf[10], eocd_buf[11]]) as u64;
    let cd_size =
        u32::from_le_bytes([eocd_buf[12], eocd_buf[13], eocd_buf[14], eocd_buf[15]]) as u64;
    let cd_offset =
        u32::from_le_bytes([eocd_buf[16], eocd_buf[17], eocd_buf[18], eocd_buf[19]]) as u64;

    let Some(cd_end) = cd_offset.checked_add(cd_size) else {
        rep.push(Finding::err("ZIP.CD", "Central Directory offset overflow"));
        return Ok(Vec::new());
    };

    if cd_end > eocd_offset {
        rep.push(Finding::err(
            "ZIP.CD",
            "Central Directory overlaps with End of Central Directory record",
        ));
        return Ok(Vec::new());
    }

    let Ok(cd_len) = usize::try_from(cd_size) else {
        rep.push(Finding::err("ZIP.CD", "Central Directory is too large"));
        return Ok(Vec::new());
    };

    let mut cd_buf = vec![0u8; cd_len];
    io.read_at(cd_offset, &mut cd_buf)?;

    let mut entries = Vec::new();
    let mut cd_pos = 0usize;
    for _ in 0..total_entries {
        let Some(cdh_end) = cd_pos.checked_add(CENTRAL_DIR_HEADER_FIXED_SIZE) else {
            rep.push(Finding::err("ZIP.CD", "Central Directory offset overflow"));
            return Ok(Vec::new());
        };
        if cdh_end > cd_buf.len() {
            rep.push(Finding::err(
                "ZIP.CD",
                "Unexpected EOF while reading Central Directory entry",
            ));
            return Ok(Vec::new());
        }

        let cdh_buf = &cd_buf[cd_pos..cdh_end];
        let sig = u32::from_le_bytes([cdh_buf[0], cdh_buf[1], cdh_buf[2], cdh_buf[3]]);
        if sig != CENTRAL_DIR_HEADER_SIG {
            rep.push(Finding::err(
                "ZIP.CD_SIG",
                "Invalid Central Directory Header signature",
            ));
            return Ok(Vec::new());
        }

        let compression_method = u16::from_le_bytes([cdh_buf[10], cdh_buf[11]]);
        let expected_crc32 =
            u32::from_le_bytes([cdh_buf[16], cdh_buf[17], cdh_buf[18], cdh_buf[19]]);
        let comp_size =
            u32::from_le_bytes([cdh_buf[20], cdh_buf[21], cdh_buf[22], cdh_buf[23]]) as u64;
        let name_len = u16::from_le_bytes([cdh_buf[28], cdh_buf[29]]) as usize;
        let extra_len = u16::from_le_bytes([cdh_buf[30], cdh_buf[31]]) as usize;
        let comment_len = u16::from_le_bytes([cdh_buf[32], cdh_buf[33]]) as usize;
        let lfh_offset =
            u32::from_le_bytes([cdh_buf[42], cdh_buf[43], cdh_buf[44], cdh_buf[45]]) as u64;

        let Some(lfh_end) = lfh_offset.checked_add(LOCAL_FILE_HEADER_FIXED_SIZE as u64) else {
            rep.push(Finding::err("ZIP.LFH", "Local File Header offset overflow"));
            return Ok(Vec::new());
        };
        if lfh_end > total_len {
            rep.push(Finding::err(
                "ZIP.LFH",
                "Local File Header offset exceeds archive bounds",
            ));
            return Ok(Vec::new());
        }

        let mut lfh_buf = [0u8; LOCAL_FILE_HEADER_FIXED_SIZE];
        io.read_at(lfh_offset, &mut lfh_buf)?;
        let lfh_sig = u32::from_le_bytes([lfh_buf[0], lfh_buf[1], lfh_buf[2], lfh_buf[3]]);
        if lfh_sig != LOCAL_FILE_HEADER_SIG {
            rep.push(Finding::err(
                "ZIP.LFH",
                "Invalid Local File Header signature",
            ));
            return Ok(Vec::new());
        }

        let lfh_name_len = u16::from_le_bytes([lfh_buf[26], lfh_buf[27]]) as u64;
        let lfh_extra_len = u16::from_le_bytes([lfh_buf[28], lfh_buf[29]]) as u64;
        let Some(data_offset) = lfh_offset
            .checked_add(LOCAL_FILE_HEADER_FIXED_SIZE as u64)
            .and_then(|v| v.checked_add(lfh_name_len))
            .and_then(|v| v.checked_add(lfh_extra_len))
        else {
            rep.push(Finding::err("ZIP.LFH", "Entry data offset overflow"));
            return Ok(Vec::new());
        };

        let Some(data_end) = data_offset.checked_add(comp_size) else {
            rep.push(Finding::err("ZIP.DATA", "Entry data offset overflow"));
            return Ok(Vec::new());
        };
        if data_end > total_len {
            rep.push(Finding::err(
                "ZIP.DATA",
                "Entry data exceeds archive bounds",
            ));
            return Ok(Vec::new());
        }

        entries.push(ZipEntryCheck {
            compression_method,
            expected_crc32,
            comp_size,
            data_offset,
        });

        let Some(next_pos) = cdh_end
            .checked_add(name_len)
            .and_then(|v| v.checked_add(extra_len))
            .and_then(|v| v.checked_add(comment_len))
        else {
            rep.push(Finding::err("ZIP.CD", "Central Directory entry overflow"));
            return Ok(Vec::new());
        };
        cd_pos = next_pos;
    }

    Ok(entries)
}

fn find_eocd<IO: RimIO + ?Sized>(io: &mut IO, total_len: u64) -> FsCheckerResult<Option<u64>> {
    let mut eocd_pos = None;
    let chunk_size = 262144usize;
    let mut scan_end = total_len;
    let mut chunk = vec![0u8; chunk_size];
    let eocd_sig_bytes = END_OF_CENTRAL_DIR_SIG.to_le_bytes();

    while scan_end >= END_OF_CENTRAL_DIR_FIXED_SIZE as u64 {
        let scan_start = scan_end.saturating_sub(chunk_size as u64);
        let cur_chunk_len = (scan_end - scan_start) as usize;
        io.read_at(scan_start, &mut chunk[..cur_chunk_len])?;

        if is_all_zeros(&chunk[..cur_chunk_len]) {
            scan_end = scan_start;
            continue;
        }

        if let Some(pos) = chunk[..cur_chunk_len]
            .windows(4)
            .rposition(|w| w == eocd_sig_bytes)
        {
            eocd_pos = Some(scan_start + pos as u64);
            break;
        }

        if scan_start == 0 {
            break;
        }

        scan_end = scan_start + (END_OF_CENTRAL_DIR_FIXED_SIZE as u64 - 1);
    }

    Ok(eocd_pos)
}

#[inline]
fn is_all_zeros(buf: &[u8]) -> bool {
    buf.iter().all(|&b| b == 0)
}
