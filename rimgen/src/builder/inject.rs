// SPDX-License-Identifier: MIT

use crate::builder::BuildEvent;
use crate::errors::{GenError, GenResult};
use crate::layout::constants::*;
use crate::layout::*;
use rimfs::FsResult;
use rimfs::core::resolver::*;
use rimfs::exfat::*;
use rimfs::ext::*;
use rimfs::fat::*;
use rimfs::ntfs::{NtfsChecker, NtfsFormatter, NtfsInjector, NtfsMeta};
use rimio::prelude::StdRimIO;
use rimio::{RimIO, RimIOExt};
use rimpart::gpt::GptEntry;
use std::path::Path;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PartitionReport {
    pub name: String,
    pub fs: Filesystem,
    pub start_lba: u64,
    pub end_lba: u64,
    pub size_bytes: u64,
    pub dirs_count: usize,
    pub files_count: usize,
    pub duration: Duration,
}

/// Format and inject files into all partitions described by layout.
pub fn format_inject_all<F: FnMut(BuildEvent)>(
    io: &mut dyn RimIO,
    layout: &Layout,
    entries: &[GptEntry],
    mut on_event: F,
) -> GenResult<Vec<PartitionReport>> {
    let mut parser = StdResolver::new();
    let mut reports = Vec::with_capacity(layout.partitions.len());
    let total_partitions = layout.partitions.len();

    for (i, part) in layout.partitions.iter().enumerate() {
        on_event(BuildEvent::PartitionStart {
            index: i,
            total: total_partitions,
            name: &part.name,
        });

        let mountpoint = &part.mountpoint.as_deref().unwrap_or("");
        let source_path = layout.base_dir.join(mountpoint);
        let node = if !mountpoint.is_empty() {
            parser.parse_tree(source_path.to_str().unwrap_or(""))?
        } else {
            FsNode::Container {
                children: vec![],
                attr: FileAttributes::new_dir(),
            }
        };

        let start_lba = entries[i].start_lba;
        let end_lba = entries[i].end_lba;
        let size_bytes = (end_lba - start_lba + 1) * DEFAULT_SECTOR_SIZE;

        let (dirs, files, duration) = match part.fs {
            Filesystem::Fat32
            | Filesystem::Fat16
            | Filesystem::Fat12
            | Filesystem::Fat8
            | Filesystem::RimFat => format_inject_fat(io, entries[i], part, &node)?,
            Filesystem::ExFat => format_inject_exfat(io, entries[i], part, &node)?,
            Filesystem::Ext4 => format_inject_ext4(io, entries[i], part, &node)?,
            Filesystem::Ntfs => format_inject_ntfs(io, entries[i], part, &node)?,
            Filesystem::Raw => format_raw(io, entries[i], part, &layout.base_dir, &mut on_event)?,
            _ => {
                return Err(GenError::UnsupportedFs(part.fs));
            }
        };

        let rep = PartitionReport {
            name: part.name.clone(),
            fs: part.fs,
            start_lba,
            end_lba,
            size_bytes,
            dirs_count: dirs,
            files_count: files,
            duration,
        };

        on_event(BuildEvent::PartitionFormatted(&rep));
        reports.push(rep);
    }

    Ok(reports)
}

/// Format + inject FAT (12/16/32/8/RimFat) partition
pub fn format_inject_fat(
    io: &mut dyn RimIO,
    entry: GptEntry,
    part: &Partition,
    node: &FsNode,
) -> FsResult<(usize, usize, Duration)> {
    let t0 = Instant::now();

    let start_lba = entry.start_lba;
    let end_lba = entry.end_lba;
    let offset = start_lba * DEFAULT_SECTOR_SIZE;
    let size_bytes = (end_lba - start_lba + 1) * DEFAULT_SECTOR_SIZE;

    io.set_offset(offset);

    let label = part.label.as_deref().unwrap_or(&part.name);

    let mut meta = match part.fs {
        Filesystem::Fat32 => FatMeta::new_fat32(size_bytes, Some(label))?,
        Filesystem::Fat16 => FatMeta::new_fat16(size_bytes, Some(label))?,
        Filesystem::Fat12 => FatMeta::new_fat12(size_bytes, Some(label))?,
        Filesystem::Fat8 => FatMeta::new_fat8(size_bytes, Some(label))?,
        Filesystem::RimFat => FatMeta::new_rimfat(size_bytes, Some(label))?,
        _ => return Err(FsError::Invalid("Unsupported FAT variant")),
    };

    if let Some(uuid_str) = &part.uuid {
        let clean = uuid_str.replace('-', "");
        if let Ok(val) = u32::from_str_radix(&clean, 16) {
            meta.volume_id = val;
        } else if let Ok(val) = uuid_str.parse::<u32>() {
            meta.volume_id = val;
        } else {
            return Err(FsError::Invalid(
                "Invalid FAT Volume ID format. Expected 32-bit hex (e.g. 'ABCD-1234')",
            ));
        }
    }

    let mut formatter = FatFormatter::new(io, &meta);
    formatter.format(false)?;

    let mut injector = FatInjector::new(io, &meta)?;
    injector.inject_tree(node)?;

    let mut checker = FatChecker::new(io, &meta);
    let _ = checker.check_all()?;

    let mut parser = FatResolver::new(io, &meta);
    let fs_root = parser.parse_tree("/*")?;
    let counts = fs_root.counts();

    Ok((counts.dirs, counts.files, t0.elapsed()))
}

/// Format + inject ExFAT partition
pub fn format_inject_exfat(
    io: &mut dyn RimIO,
    entry: GptEntry,
    part: &Partition,
    node: &FsNode,
) -> FsResult<(usize, usize, Duration)> {
    let t0 = Instant::now();

    let start_lba = entry.start_lba;
    let end_lba = entry.end_lba;
    let offset = start_lba * DEFAULT_SECTOR_SIZE;
    let size_bytes = (end_lba - start_lba + 1) * DEFAULT_SECTOR_SIZE;

    io.set_offset(offset);

    let label = part.label.as_deref().unwrap_or(&part.name);
    let mut meta = ExFatMeta::new(size_bytes, Some(label))?;

    if let Some(uuid_str) = &part.uuid {
        let clean = uuid_str.replace('-', "");
        if let Ok(val) = u32::from_str_radix(&clean, 16) {
            meta.volume_id = val;
        } else if let Ok(uuid) = uuid_str.parse::<Uuid>() {
            meta.volume_guid = Some(*uuid.as_bytes());
        } else {
            return Err(FsError::Invalid(
                "Invalid ExFAT UUID format. Expected 32-bit hex or UUID.",
            ));
        }
    }

    let mut formatter = ExFatFormatter::new(io, &meta);
    formatter.format(false)?;

    let mut injector = ExFatInjector::new(io, &meta)?;
    injector.inject_tree(node)?;

    let mut checker = ExFatChecker::new(io, &meta);
    let _ = checker.check_all()?;

    let mut parser = ExFatResolver::new(io, &meta);
    let fs_root = parser.parse_tree("/*")?;
    let counts = fs_root.counts();

    Ok((counts.dirs, counts.files, t0.elapsed()))
}

/// Format + inject Ext4 partition
pub fn format_inject_ext4(
    io: &mut dyn RimIO,
    entry: GptEntry,
    part: &Partition,
    node: &FsNode,
) -> FsResult<(usize, usize, Duration)> {
    let t0 = Instant::now();

    let start_lba = entry.start_lba;
    let end_lba = entry.end_lba;
    let offset = start_lba * DEFAULT_SECTOR_SIZE;
    let size_bytes = (end_lba - start_lba + 1) * DEFAULT_SECTOR_SIZE;

    io.set_offset(offset);

    let label = part.label.as_deref().unwrap_or(&part.name);
    let mut meta = ExtMeta::new(size_bytes, Some(label));

    if let Some(uuid_str) = &part.uuid {
        if let Ok(uuid) = uuid_str.parse::<Uuid>() {
            meta.volume_id = *uuid.as_bytes();
        } else {
            return Err(FsError::Invalid("Invalid EXT4 UUID format. Expected UUID."));
        }
    }

    let mut formatter = ExtFormatter::new(io, &meta);
    formatter.format(false)?;

    let mut injector = ExtInjector::new(io, &meta);
    injector.inject_tree(node)?;

    let mut checker = ExtChecker::new(io, &meta);
    let _ = checker.check_all()?;

    let counts = node.counts();

    Ok((counts.dirs, counts.files, t0.elapsed()))
}

/// Format + inject NTFS partition
pub fn format_inject_ntfs(
    io: &mut dyn RimIO,
    entry: GptEntry,
    part: &Partition,
    node: &FsNode,
) -> FsResult<(usize, usize, Duration)> {
    let t0 = Instant::now();

    let start_lba = entry.start_lba;
    let end_lba = entry.end_lba;
    let offset = start_lba * DEFAULT_SECTOR_SIZE;
    let size_bytes = (end_lba - start_lba + 1) * DEFAULT_SECTOR_SIZE;

    io.set_offset(offset);

    let label = part.label.as_deref().unwrap_or(&part.name);
    let meta = NtfsMeta::new(size_bytes, Some(label))?;

    let mut formatter = NtfsFormatter::new(io, &meta);
    formatter.format(false)?;

    let mut injector = NtfsInjector::new(io, &meta)?;
    injector.inject_tree(node)?;

    let mut checker = NtfsChecker::new(io, &meta);
    let _ = checker.check_all()?;

    let counts = node.counts();

    Ok((counts.dirs, counts.files, t0.elapsed()))
}

/// Format + write raw partition payload
pub fn format_raw<F: FnMut(BuildEvent)>(
    io: &mut dyn RimIO,
    entry: GptEntry,
    part: &Partition,
    base_dir: &Path,
    mut on_event: F,
) -> GenResult<(usize, usize, Duration)> {
    let t0 = Instant::now();

    let start_lba = entry.start_lba;
    let end_lba = entry.end_lba;
    let offset = start_lba * DEFAULT_SECTOR_SIZE;
    let size_bytes = (end_lba - start_lba + 1) * DEFAULT_SECTOR_SIZE;
    let max_size = size_bytes as usize;

    io.set_offset(offset);

    let mut files_written = 0;

    if let Some(payload_relative) = &part.payload {
        let payload_path = base_dir.join(payload_relative);
        let mut file = std::fs::File::open(&payload_path)?;

        let metadata = file.metadata()?;
        let file_size = metadata.len();
        if file_size > size_bytes {
            return Err(GenError::PayloadTooLarge {
                path: payload_path.display().to_string(),
                part_name: part.name.clone(),
                payload_bytes: file_size,
                part_bytes: size_bytes,
            });
        }

        let mut file_io = StdRimIO::new(&mut file);
        io.copy_from_with_progress(&mut file_io, 0, 0, file_size, |current, total| {
            on_event(BuildEvent::PayloadProgress {
                current_bytes: current,
                total_bytes: total,
            });
        })?;

        files_written = 1;
    } else {
        io.zero_fill(0, max_size)?;
    }

    Ok((0, files_written, t0.elapsed()))
}
