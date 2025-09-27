// SPDX-License-Identifier: MIT

use crate::layout::constants::*;
use crate::layout::*;
use crate::out::helpers::{partition_to_gpt_partition_entry, size_to_sectors};
use crate::out::target::{DryRunMode, TargetImage};
use crate::utils;
use rimfs::exfat::*;
use rimfs::fat32::*;
use rimpart::gpt::GptEntry;
use rimpart::scanner::DiskScanOptions;
use std::fs::File;
use std::path::Path;
use std::time::Instant;
use std::vec;
use uuid::Uuid;

#[cfg(feature = "host-scripts")]
use crate::host;

/// Entry point — create disk image
pub fn create(
    layout: &Layout,
    output: &Path,
    truncate: &bool,
    dry_mode: DryRunMode,
) -> anyhow::Result<()> {
    let t0 = Instant::now();
    let total_sectors = calculate_total_disk_sectors(layout);
    let total_bytes = total_sectors * SECTOR_SIZE;

    // 1) PLAN : calculs purs (toujours faits)
    let mut partition_entries = vec![];
    let align = rimpart::gpt::align_lba_1m(SECTOR_SIZE);
    let mut start = align;
    for part in &layout.partitions {
        let sectors = size_to_sectors(&part.size);
        let end = start + sectors - 1;
        if end >= total_sectors {
            anyhow::bail!(
                "Partition '{}' does not fit ({} > {})",
                part.name,
                end,
                total_sectors
            );
        }
        partition_entries.push(partition_to_gpt_partition_entry(part, start, end)?);
        start = rimpart::gpt::align_up(end, align);
    }

    // Mode Plan => on s’arrête après le plan et les checks logiques
    if matches!(dry_mode, DryRunMode::Plan) {
        crate::log_info!("Partition table");
        for (idx, e) in partition_entries.iter().enumerate() {
            let bytes = (e.end_lba - e.start_lba + 1) * SECTOR_SIZE;
            let name = e.name;
            let start_lba = e.start_lba;
            let end_lba = e.end_lba;
            crate::log_info!(
                "#{:02} {:?}  {:>8}–{:>8} ({}  aligned {})",
                idx,
                name,
                start_lba,
                end_lba,
                utils::pretty_bytes(bytes),
                ALIGNMENT
            );
        }
        crate::log_info!("Dry-run (plan) only: GPT not written, no formatting performed.");
        return Ok(());
    }

    // 2) Ouvrir la cible (tempfile sparse ou fichier final)
    let mut target = TargetImage::open(output, total_bytes, dry_mode)?;

    // 3) Écritures GPT réelles
    {
        let disk_guid = *Uuid::new_v4().as_bytes();
        let mut io = target.as_io()?;

        rimpart::mbr::write_mbr_protective(&mut io, total_sectors)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        rimpart::gpt::write_gpt(&mut io, &partition_entries, total_sectors, disk_guid)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        if *truncate
            && let Some(rep) = rimpart::truncate_image_custom_sector(
                &mut io,
                &partition_entries,
                total_sectors,
                SECTOR_SIZE,
            )
            .map_err(|e| anyhow::anyhow!("{}", e))?
        {
            crate::log_verbose!(
                "truncated • total={} • used={} • saved={}",
                utils::pretty_bytes(rep.total_bytes),
                utils::pretty_bytes(rep.used_bytes),
                utils::pretty_bytes(rep.saved_bytes)
            );
        }

        rimpart::validate_full_disk(&mut io).map_err(|e| anyhow::anyhow!("{}", e))?;
        let info = rimpart::scan_disk(
            &mut io,
            DiskScanOptions::new().with_sector_size(SECTOR_SIZE),
        )
        .map_err(|e| anyhow::anyhow!("{}", e))?;
        crate::log_verbose!("{info}");
    }

    crate::log_info!("GPT written in {:.2}s", t0.elapsed().as_secs_f32());

    format_inject(layout, &mut target)?;

    Ok(())
}

/// Format + inject content into partitions
fn format_inject(layout: &Layout, target: &mut TargetImage) -> anyhow::Result<()> {
    let path = target.path.clone();
    let mode = target.mode;
    let mut io = target.as_io()?;
    let (_hdr, entries) = rimpart::gpt::read_gpt_with_sector(&mut io, SECTOR_SIZE, true)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let mut parser = StdResolver::new();

    for (i, part) in layout.partitions.iter().enumerate() {
        let mountpoint = &part.mountpoint.as_deref().unwrap_or("");

        let source_path = layout.base_dir.join(&mountpoint);
        let node = if !mountpoint.is_empty() {
            parser
                .parse_tree(source_path.to_str().unwrap())
                .map_err(|e| anyhow::anyhow!("{}", e))?
        } else {
            FsNode::Container {
                children: vec![],
                attr: FileAttributes::new_dir(),
            }
        };
        match part.fs {
            Filesystem::Fat32 => {
                format_inject_fat32(&mut io, entries[i], part, &node)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
            }
            Filesystem::ExFat => {
                format_inject_exfat(&mut io, entries[i], part, &node)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
            }
            Filesystem::Raw => {
                format_raw(&mut io, entries[i], part).map_err(|e| anyhow::anyhow!("{}", e))?;
            }
            // Filesystem::Ext4 => {
            //     format_inject_ext4(&mut file, &gpt, i, layout, part, &mut parser)?;
            // }
            _ => {
                #[cfg(feature = "host-scripts")]
                {
                    crate::log_info!(
                        "Filesystem {:?} unsupported natively. Trying host-script for \"{}\"…",
                        part.fs,
                        part.name
                    );

                    let mut partition = part.clone();
                    partition.index = Some(i);
                    // Layout réduit à une seule partition pour le fallback host
                    let single = Layout {
                        base_dir: layout.base_dir.clone(),
                        partitions: vec![partition.clone()],
                    };

                    let t0 = Instant::now();

                    // Le host-script utilisera le GPT existant et n'agira que sur cette part
                    if let Some(err) = host::format_inject_host(&single, &path, mode).err() {
                        anyhow::bail!(
                            "Host-script failed for \"{}\" ({}). Hint: run on a supported OS or install required tools.",
                            part.name,
                            err
                        );
                    }

                    let dt = t0.elapsed().as_secs_f32();

                    crate::log_info!(
                        "\"{}\" {} formatted and injected using host-script in {dt:.2}s",
                        part.name,
                        part.fs
                    );
                }
                #[cfg(not(feature = "host-scripts"))]
                {
                    anyhow::bail!(
                        "Filesystem {:?} unsupported on this platform and no host-scripts enabled. Can't write \"{}\".",
                        part.fs,
                        part.name
                    );
                }
            }
        }
    }

    Ok(())
}

/// Format + inject FAT32 partition
fn format_inject_fat32(
    io: &mut dyn BlockIO,
    entry: GptEntry,
    part: &Partition,
    node: &FsNode,
) -> FsResult {
    let t0 = Instant::now();

    let start_lba = entry.start_lba;
    let end_lba = entry.end_lba;
    let offset = start_lba * SECTOR_SIZE;
    let size_bytes = (end_lba - start_lba + 1) * SECTOR_SIZE;

    io.set_offset(offset);

    let meta = Fat32Meta::new(size_bytes, Some(&part.name));

    let mut formatter = Fat32Formatter::new(io, &meta);
    formatter.format(false)?;

    let mut allocator = Fat32Allocator::new(&meta);
    let mut injector = Fat32Injector::new(io, &mut allocator, &meta);
    injector.inject_tree(node)?;

    let mut checker = Fat32Checker::new(io, &meta);
    let report = checker.check_all()?;

    if (report.has_error()) {
        crate::log_normal!("{}", report.errors_only());
    }

    let mut parser = Fat32Resolver::new(io, &meta);
    let fs_root = parser.parse_tree("/*")?;
    let counts = fs_root.counts();

    crate::log_verbose!("On disk \n{}", fs_root);

    let dt = t0.elapsed().as_secs_f32();

    crate::log_info!(
        "FAT32 part \"{}\" formatted and {counts} injected using RIM in {dt:.2}s",
        part.name,
    );

    Ok(())
}

fn format_inject_exfat(
    io: &mut dyn BlockIO,
    entry: GptEntry,
    part: &Partition,
    node: &FsNode,
) -> FsResult {
    let t0 = Instant::now();

    let start_lba = entry.start_lba;
    let end_lba = entry.end_lba;
    let offset = start_lba * SECTOR_SIZE;
    let size_bytes = (end_lba - start_lba + 1) * SECTOR_SIZE;

    io.set_offset(offset);

    let meta = ExFatMeta::new(size_bytes, Some(&part.name));

    let mut formatter = ExFatFormatter::new(io, &meta);
    formatter.format(false)?;

    let mut allocator = ExFatAllocator::new(&meta);
    let mut injector = ExFatInjector::new(io, &mut allocator, &meta);
    injector.inject_tree(node)?;

    let mut checker = ExFatChecker::new(io, &meta);
    let report = checker.check_all()?;

    if (report.has_error()) {
        crate::log_normal!("{}", report.errors_only());
    }

    let mut parser = ExFatResolver::new(io, &meta);
    let fs_root = parser.parse_tree("/*")?;
    let counts = fs_root.counts();

    crate::log_verbose!("On disk \n{}", fs_root);

    let dt = t0.elapsed().as_secs_f32();

    crate::log_info!(
        "\"{}\" exFAT formatted and {counts} injected using RIM in {dt:.2}s",
        part.name,
    );

    Ok(())
}

fn format_raw(io: &mut dyn BlockIO, entry: GptEntry, part: &Partition) -> FsResult {
    let t0 = Instant::now();

    let start_lba = entry.start_lba;
    let end_lba = entry.end_lba;
    let offset = start_lba * SECTOR_SIZE;
    let size_bytes = (end_lba - start_lba + 1) * SECTOR_SIZE;

    io.set_offset(offset);

    io.zero_fill(0, size_bytes as usize)?;

    let dt = t0.elapsed().as_secs_f32();

    crate::log_info!(
        "\"{}\" RAW formatted (zero-filled) using RIM in {dt:.2}s",
        part.name,
    );

    Ok(())
}

/// Calculate total disk sectors needed
fn calculate_total_disk_sectors(layout: &Layout) -> u64 {
    layout
        .partitions
        .iter()
        .map(|p| size_to_sectors(&p.size) + ALIGNMENT)
        .sum::<u64>()
        + ALIGNMENT
}
