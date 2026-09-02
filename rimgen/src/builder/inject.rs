// SPDX-License-Identifier: MIT

use crate::builder::BuildEvent;
use crate::errors::{GenError, GenResult};
use crate::layout::constants::*;
use crate::layout::*;
use ::core::time::Duration;
use alloc::string::String;
use alloc::vec::Vec;
use rimfs::core::allocator::FsHandle;
use rimfs::core::injector::FsTreeInjector;
use rimfs::core::resolver::*;
#[cfg(feature = "exfat")]
use rimfs::exfat::*;
#[cfg(feature = "ext")]
use rimfs::ext::*;
#[cfg(feature = "fat")]
use rimfs::fat::*;
#[cfg(all(feature = "ntfs", feature = "std"))]
use rimfs::ntfs::NtfsChecker;
#[cfg(feature = "ntfs")]
use rimfs::ntfs::{NtfsFormatter, NtfsHandle, NtfsInjector, NtfsMeta};
use rimio::{RimIO, RimWriteExt};
use rimpart::gpt::GptEntry;
#[cfg(feature = "std")]
use std::time::Instant;
#[cfg(any(feature = "exfat", feature = "ext", feature = "ntfs"))]
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
    pub symlinks_count: usize,
    pub duration: Duration,
}

#[derive(Clone, Copy)]
struct PartitionSpan {
    offset: u64,
    size_bytes: u64,
}

fn merge_counts(dst: &mut FsNodeCounts, src: FsNodeCounts) {
    dst.dirs += src.dirs;
    dst.files += src.files;
    dst.symlinks += src.symlinks;
    dst.bytes += src.bytes;
}

fn inject_partition_sources<Handle, Injector>(
    injector: &mut Injector,
    part: &mut Partition<'_>,
) -> FsResult<FsNodeCounts>
where
    Handle: FsHandle,
    Injector: FsTreeInjector<Handle>,
{
    let mut counts = FsNodeCounts::default();

    if let Some(ref mut root) = part.root {
        injector.inject_tree(root)?;
        merge_counts(&mut counts, root.counts());
    }

    #[cfg(feature = "std")]
    if let Some(source_path) = &part.source_mountpoint {
        let mut resolver = rimfs::core::StdResolver::new();
        let path = source_path.to_str().ok_or(rimfs::FsError::Invalid(
            "Mountpoint path is not valid UTF-8",
        ))?;
        let source_counts = injector.inject_tree_from_resolver(&mut resolver, path)?;
        merge_counts(&mut counts, source_counts);
    }

    Ok(counts)
}

impl PartitionSpan {
    fn from_entry(entry: GptEntry) -> Self {
        let sectors = entry.end_lba - entry.start_lba + 1;
        Self {
            offset: entry.start_lba * DEFAULT_SECTOR_SIZE,
            size_bytes: sectors * DEFAULT_SECTOR_SIZE,
        }
    }
}

/// Format and inject files into all partitions described by a layout.
pub fn format_inject_resolved_all<F: for<'a> FnMut(BuildEvent<'a>)>(
    io: &mut dyn RimIO,
    layout: &mut Layout<'_>,
    entries: &[GptEntry],
    mut on_event: F,
) -> GenResult<Vec<PartitionReport>> {
    let mut reports = Vec::with_capacity(layout.partitions.len());
    let total_partitions = layout.partitions.len();

    for (i, part) in layout.partitions.iter_mut().enumerate() {
        on_event(BuildEvent::PartitionStart {
            index: i,
            total: total_partitions,
            name: &part.name,
        });

        let entry = entries[i];
        let span = PartitionSpan::from_entry(entry);

        let (counts, duration) = match part.fs {
            #[cfg(feature = "fat")]
            Filesystem::Fat32
            | Filesystem::Fat16
            | Filesystem::Fat12
            | Filesystem::Fat8
            | Filesystem::RimFat => format_inject_fat(io, entry, part)?,
            #[cfg(feature = "exfat")]
            Filesystem::ExFat => format_inject_exfat(io, entry, part)?,
            #[cfg(feature = "ext")]
            Filesystem::Ext4 => format_inject_ext4(io, entry, part)?,
            #[cfg(feature = "ntfs")]
            Filesystem::Ntfs => format_inject_ntfs(io, entry, part)?,
            Filesystem::Raw => format_raw(io, entry, part, &mut on_event)?,
            _ => {
                return Err(GenError::UnsupportedFs(part.fs));
            }
        };

        let rep = PartitionReport {
            name: part.name.clone(),
            fs: part.fs,
            start_lba: entry.start_lba,
            end_lba: entry.end_lba,
            size_bytes: span.size_bytes,
            dirs_count: counts.dirs,
            files_count: counts.files,
            symlinks_count: counts.symlinks,
            duration,
        };

        on_event(BuildEvent::PartitionFormatted(&rep));
        reports.push(rep);
    }

    Ok(reports)
}

/// Format + inject FAT (12/16/32/8/RimFat) partition
#[cfg(feature = "fat")]
pub fn format_inject_fat(
    io: &mut dyn RimIO,
    entry: GptEntry,
    part: &mut Partition<'_>,
) -> FsResult<(FsNodeCounts, Duration)> {
    #[cfg(feature = "std")]
    let t0 = Instant::now();

    let span = PartitionSpan::from_entry(entry);
    io.set_offset(span.offset);

    let label = part.label.as_deref().unwrap_or(&part.name);
    let mut meta = match part.fs {
        Filesystem::Fat32 => FatMeta::new_fat32(span.size_bytes, Some(label))?,
        Filesystem::Fat16 => FatMeta::new_fat16(span.size_bytes, Some(label))?,
        Filesystem::Fat12 => FatMeta::new_fat12(span.size_bytes, Some(label))?,
        Filesystem::Fat8 => FatMeta::new_fat8(span.size_bytes, Some(label))?,
        Filesystem::RimFat => FatMeta::new_rimfat(span.size_bytes, Some(label))?,
        _ => return Err(rimfs::FsError::Invalid("Unsupported FAT variant")),
    };

    if let Some(uuid_str) = &part.uuid {
        let clean = uuid_str.replace('-', "");
        if let Ok(val) = u32::from_str_radix(&clean, 16) {
            meta.volume_id = val;
        } else if let Ok(val) = uuid_str.parse::<u32>() {
            meta.volume_id = val;
        } else {
            return Err(rimfs::FsError::Invalid(
                "Invalid FAT Volume ID format. Expected 32-bit hex (e.g. 'ABCD-1234')",
            ));
        }
    }

    let mut formatter = FatFormatter::new(io, &meta);
    formatter.format(false)?;

    let mut injector = FatInjector::new(io, &meta)?;
    let counts = inject_partition_sources::<FatHandle, _>(&mut injector, part)?;

    #[cfg(feature = "std")]
    {
        let mut checker = FatChecker::new(io, &meta);
        let _ = checker.check_all()?;
    }

    #[cfg(feature = "std")]
    let duration = t0.elapsed();
    #[cfg(not(feature = "std"))]
    let duration = Duration::ZERO;

    Ok((counts, duration))
}

/// Format + inject ExFAT partition
#[cfg(feature = "exfat")]
pub fn format_inject_exfat(
    io: &mut dyn RimIO,
    entry: GptEntry,
    part: &mut Partition<'_>,
) -> FsResult<(FsNodeCounts, Duration)> {
    #[cfg(feature = "std")]
    let t0 = Instant::now();

    let span = PartitionSpan::from_entry(entry);
    io.set_offset(span.offset);

    let label = part.label.as_deref().unwrap_or(&part.name);
    let mut meta = ExFatMeta::new(span.size_bytes, Some(label))?;

    if let Some(uuid_str) = &part.uuid {
        let clean = uuid_str.replace('-', "");
        if let Ok(val) = u32::from_str_radix(&clean, 16) {
            meta.volume_id = val;
        } else if let Ok(uuid) = uuid_str.parse::<Uuid>() {
            meta.volume_guid = Some(*uuid.as_bytes());
        } else {
            return Err(rimfs::FsError::Invalid(
                "Invalid ExFAT UUID format. Expected 32-bit hex or UUID.",
            ));
        }
    }

    let mut formatter = ExFatFormatter::new(io, &meta);
    formatter.format(false)?;

    let mut injector = ExFatInjector::new(io, &meta)?;
    let counts = inject_partition_sources::<ExFatHandle, _>(&mut injector, part)?;

    #[cfg(feature = "std")]
    {
        let mut checker = ExFatChecker::new(io, &meta);
        let _ = checker.check_all()?;
    }

    #[cfg(feature = "std")]
    let duration = t0.elapsed();
    #[cfg(not(feature = "std"))]
    let duration = Duration::ZERO;

    Ok((counts, duration))
}

/// Format + inject Ext4 partition
#[cfg(feature = "ext")]
pub fn format_inject_ext4(
    io: &mut dyn RimIO,
    entry: GptEntry,
    part: &mut Partition<'_>,
) -> FsResult<(FsNodeCounts, Duration)> {
    #[cfg(feature = "std")]
    let t0 = Instant::now();

    let span = PartitionSpan::from_entry(entry);
    io.set_offset(span.offset);

    let label = part.label.as_deref().unwrap_or(&part.name);
    let mut meta = ExtMeta::new(span.size_bytes, Some(label))?;

    if let Some(uuid_str) = &part.uuid {
        if let Ok(uuid) = uuid_str.parse::<Uuid>() {
            meta.volume_id = *uuid.as_bytes();
        } else {
            return Err(rimfs::FsError::Invalid(
                "Invalid EXT4 UUID format. Expected UUID.",
            ));
        }
    }

    let mut formatter = ExtFormatter::new(io, &meta);
    formatter.format(false)?;

    let mut injector = ExtInjector::new(io, &meta)?;
    let counts = inject_partition_sources::<ExtHandle, _>(&mut injector, part)?;

    #[cfg(feature = "std")]
    {
        let mut checker = ExtChecker::new(io, &meta);
        let _ = checker.check_all()?;
    }

    #[cfg(feature = "std")]
    let duration = t0.elapsed();
    #[cfg(not(feature = "std"))]
    let duration = Duration::ZERO;

    Ok((counts, duration))
}

/// Format + inject NTFS partition
#[cfg(feature = "ntfs")]
pub fn format_inject_ntfs(
    io: &mut dyn RimIO,
    entry: GptEntry,
    part: &mut Partition<'_>,
) -> FsResult<(FsNodeCounts, Duration)> {
    #[cfg(feature = "std")]
    let t0 = Instant::now();

    let span = PartitionSpan::from_entry(entry);
    io.set_offset(span.offset);

    let label = part.label.as_deref().unwrap_or(&part.name);
    let mut meta = NtfsMeta::new(span.size_bytes, Some(label))?;

    if let Some(uuid_str) = &part.uuid {
        let clean = uuid_str.replace('-', "");
        if let Ok(val) = u64::from_str_radix(&clean, 16) {
            meta.volume_serial = val;
        } else if let Ok(val) = uuid_str.parse::<u64>() {
            meta.volume_serial = val;
        } else if let Ok(uuid) = uuid_str.parse::<Uuid>() {
            let bytes: [u8; 8] = uuid.as_bytes()[..8].try_into().unwrap();
            meta.volume_serial = u64::from_le_bytes(bytes);
        } else {
            return Err(rimfs::FsError::Invalid(
                "Invalid NTFS Volume Serial format. Expected 64-bit/32-bit hex (e.g. '1234-5678') or UUID.",
            ));
        }
    }

    let mut formatter = NtfsFormatter::new(io, &meta);
    formatter.format(false)?;

    let mut injector = NtfsInjector::new(io, &meta)?;
    let counts = inject_partition_sources::<NtfsHandle, _>(&mut injector, part)?;

    #[cfg(feature = "std")]
    {
        let mut checker = NtfsChecker::new(io, &meta);
        let _ = checker.check_all()?;
    }

    #[cfg(feature = "std")]
    let duration = t0.elapsed();
    #[cfg(not(feature = "std"))]
    let duration = Duration::ZERO;

    Ok((counts, duration))
}

/// Format + write raw partition payload
pub fn format_raw<F: for<'a> FnMut(BuildEvent<'a>)>(
    io: &mut dyn RimIO,
    entry: GptEntry,
    part: &mut Partition<'_>,
    mut on_event: F,
) -> GenResult<(FsNodeCounts, Duration)> {
    #[cfg(feature = "std")]
    let t0 = Instant::now();

    let span = PartitionSpan::from_entry(entry);
    let max_size =
        usize::try_from(span.size_bytes).map_err(|_| GenError::Other("partition too large"))?;

    io.set_offset(span.offset);

    let mut files_written = 0;

    if let Some(ref mut src) = part.raw_source {
        let raw_size = part.raw_size;
        if raw_size > span.size_bytes {
            return Err(GenError::PayloadTooLarge {
                path: part.name.clone(),
                part_name: part.name.clone(),
                payload_bytes: raw_size,
                part_bytes: span.size_bytes,
            });
        }

        let mut scratch = [0u8; 64 * 1024];
        rimio::copy_range(src.as_mut(), io, 0, 0, raw_size, &mut scratch)?;

        on_event(BuildEvent::PayloadProgress {
            current_bytes: raw_size,
            total_bytes: raw_size,
        });

        files_written = 1;
    } else {
        io.zero_fill(0, max_size)?;
    }

    #[cfg(feature = "std")]
    let duration = t0.elapsed();
    #[cfg(not(feature = "std"))]
    let duration = Duration::ZERO;

    Ok((
        FsNodeCounts {
            files: files_written,
            ..FsNodeCounts::default()
        },
        duration,
    ))
}
