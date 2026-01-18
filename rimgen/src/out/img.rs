use crate::layout::constants::*;
use crate::layout::*;
use crate::out::helpers::{partition_to_gpt_partition_entry, size_to_sectors};
use crate::out::target::{DryRunMode, TargetImage};
use crate::utils;
use colored::Colorize;
use rimfs::core::FsError;
use rimfs::exfat::*;
use rimfs::fat32::*;
use rimpart::gpt::GptEntry;
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

    // PLAN: pure calculations (always performed)
    let mut partition_entries = vec![];

    // Determine alignment (default 1MB = 2048 sectors)
    let align_sectors = if let Some(disk) = &layout.disk {
        if let Some(align_str) = &disk.alignment {
            parse_alignment_sectors(align_str)?
        } else {
            rimpart::gpt::align_lba_1m(SECTOR_SIZE)
        }
    } else {
        rimpart::gpt::align_lba_1m(SECTOR_SIZE)
    };

    let mut start = align_sectors;

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
        start = rimpart::gpt::align_up(end, align_sectors);
    }

    // Plan mode => stop after the plan and logical checks
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
                format!("{} sectors", align_sectors)
            );
        }
        crate::log_info!("Dry-run (plan) only: GPT not written, no formatting performed.");
        return Ok(());
    }

    // Open target (sparse tempfile or final file)
    let mut target = TargetImage::open(output, total_bytes, dry_mode)?;

    // Real GPT writes
    {
        let disk_guid = if let Some(disk) = &layout.disk {
            if let Some(guid) = disk.guid {
                *guid.as_bytes()
            } else {
                *Uuid::new_v4().as_bytes()
            }
        } else {
            *Uuid::new_v4().as_bytes()
        };

        let mut io = target.as_io()?;

        rimpart::mbr::write_mbr_protective(&mut io, total_sectors)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        rimpart::gpt::write_gpt_from_entries(&mut io, &partition_entries, total_sectors, disk_guid)
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
        let info = rimpart::scan_disk_with_sector(&mut io, SECTOR_SIZE)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        crate::log_verbose!("{info}");
    }

    crate::log_info!(
        "GPT written in {}s",
        format!("{:.2}", t0.elapsed().as_secs_f32()).yellow()
    );

    format_inject(layout, &mut target)?;

    Ok(())
}

/// Format + inject content into partitions
fn format_inject(layout: &Layout, target: &mut TargetImage) -> anyhow::Result<()> {
    let path = target.path.clone();
    let mode = target.mode;
    let mut io = target.as_io()?;
    let (_hdr, entries) = rimpart::gpt::read_gpt_with_sector(&mut io, SECTOR_SIZE)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    crate::log_info!(
        "Starting formatting of {} partitions...",
        layout.partitions.len()
    );

    let multi_progress = indicatif::MultiProgress::new();
    let sty = indicatif::ProgressStyle::with_template(
        "{spinner:.green} [{elapsed_precise}] {bar:40.white} {pos}/{len} (ETA {eta}) {msg}",
    )
    .unwrap()
    .progress_chars("█░░");

    let pb = multi_progress.add(indicatif::ProgressBar::new(layout.partitions.len() as u64));
    pb.set_style(sty);
    pb.set_message("Formatting partitions");

    // Use StdResolver from rimfs
    let mut parser = rimfs::core::StdResolver::new();

    for (i, part) in layout.partitions.iter().enumerate() {
        let mountpoint = &part.mountpoint.as_deref().unwrap_or("");
        pb.set_message(format!("Partition {}/{}", i + 1, layout.partitions.len()));

        let source_path = layout.base_dir.join(mountpoint);
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
        let res = pb.suspend(|| {
            match part.fs {
                Filesystem::Fat32 => {
                    format_inject_fat32(&mut io, entries[i], part, &node)
                        .map_err(|e| anyhow::anyhow!("{}", e))
                }
                Filesystem::ExFat => {
                    format_inject_exfat(&mut io, entries[i], part, &node)
                        .map_err(|e| anyhow::anyhow!("{}", e))
                }
                Filesystem::Raw => format_raw(&mut io, entries[i], part, &layout.base_dir),
                Filesystem::Ext4 => {
                    format_inject_ext4(&mut io, entries[i], part, &node)
                        .map_err(|e| anyhow::anyhow!("{}", e))
                }
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
                        // Layout reduced to a single partition for host fallback
                        let single = Layout {
                            base_dir: layout.base_dir.clone(),
                            partitions: vec![partition.clone()],
                            disk: layout.disk.clone(),
                        };

                        let t0 = Instant::now();

                        // The host script will use the existing GPT and only act on this partition
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
                        Ok(())
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
        });
        res?;
        pb.inc(1);
    }
    pb.finish_and_clear();

    Ok(())
}

/// Format + inject FAT32 partition
fn format_inject_fat32(
    io: &mut dyn RimIO,
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

    let label = part.label.as_deref().unwrap_or(&part.name);
    let mut meta = Fat32Meta::new(size_bytes, Some(label))?;

    if let Some(uuid_str) = &part.uuid {
        // FAT32 only supports 32-bit serial (Volume ID)
        // Try to parse as u32 (dec or hex).
        // Support hex string "1234-ABCD" format common in FAT.
        let clean = uuid_str.replace('-', "");
        if let Ok(val) = u32::from_str_radix(&clean, 16) {
            meta.volume_id = val;
        } else if let Ok(val) = uuid_str.parse::<u32>() {
            meta.volume_id = val;
        } else {
            // FAT32 does not support UUIDs, only Volume ID (u32).
            // We fail here to be strict about the configuration.
            return Err(FsError::Invalid(
                "Invalid FAT32 UUID format. Expected 32-bit hex (e.g. 'ABCD-1234')",
            ));
        }
    }

    let mut formatter = Fat32Formatter::new(io, &meta);
    formatter.format(false)?;

    let mut allocator = Fat32Allocator::new(&meta);
    let mut injector = Fat32Injector::new(io, &mut allocator, &meta);
    injector.inject_tree(node)?;

    let mut checker = Fat32Checker::new(io, &meta);
    let report = checker.check_all()?;

    if report.has_error() {
        crate::log_normal!("{}", report.errors_only());
    }

    let mut parser = Fat32Resolver::new(io, &meta);
    let fs_root = parser.parse_tree("/*")?;
    let counts = fs_root.counts();

    crate::log_verbose!("On disk \n{}", fs_root);

    let dt = t0.elapsed().as_secs_f32();

    crate::log_info!(
        "\"{}\" formatted in {} and {} injected using RIM in {}s",
        part.name.bold(),
        "FAT32".green().bold(),
        counts.to_string().cyan(),
        format!("{dt:.2}").yellow()
    );

    Ok(())
}

fn format_inject_exfat(
    io: &mut dyn RimIO,
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

    let label = part.label.as_deref().unwrap_or(&part.name);
    let mut meta = ExFatMeta::new(size_bytes, Some(label))?;

    if let Some(uuid_str) = &part.uuid {
        // ExFAT supports Serial (u32) AND GUID (128-bit).
        // If fits in u32 -> Serial. If UUID -> GUID.
        let clean = uuid_str.replace('-', "");
        if let Ok(val) = u32::from_str_radix(&clean, 16) {
            meta.volume_id = val;
            // ExFAT supports both Serial and GUID.
            // If u32 is parsed, we set volume_id. The GUID remains default (or zero) unless specified.
        } else if let Ok(uuid) = uuid_str.parse::<Uuid>() {
            meta.volume_guid = Some(*uuid.as_bytes());
            meta.volume_guid = Some(*uuid.as_bytes());
        } else {
            return Err(FsError::Invalid(
                "Invalid ExFAT UUID format. Expected 32-bit hex or UUID.",
            ));
        }
    }

    let mut formatter = ExFatFormatter::new(io, &meta);
    formatter.format(false)?;

    let mut allocator = ExFatAllocator::new(&meta);
    let mut injector = ExFatInjector::new(io, &mut allocator, &meta)?;
    injector.inject_tree(node)?;

    let mut checker = ExFatChecker::new(io, &meta);
    let report = checker.check_all()?;

    if report.has_error() {
        crate::log_normal!("{}", report.errors_only());
    }

    let mut parser = ExFatResolver::new(io, &meta);
    let fs_root = parser.parse_tree("/*")?;
    let counts = fs_root.counts();

    crate::log_verbose!("On disk \n{}", fs_root);

    let dt = t0.elapsed().as_secs_f32();

    crate::log_info!(
        "\"{}\" formatted in {} and {} injected using RIM in {}s",
        part.name.bold(),
        "exFAT".cyan().bold(),
        counts.to_string().cyan(),
        format!("{dt:.2}").yellow()
    );

    Ok(())
}

fn format_inject_ext4(
    io: &mut dyn RimIO,
    entry: GptEntry,
    part: &Partition,
    node: &FsNode,
) -> FsResult {
    use rimfs::ext4::*;

    let t0 = Instant::now();

    let start_lba = entry.start_lba;
    let end_lba = entry.end_lba;
    let offset = start_lba * SECTOR_SIZE;
    let size_bytes = (end_lba - start_lba + 1) * SECTOR_SIZE;

    io.set_offset(offset);

    let label = part.label.as_deref().unwrap_or(&part.name);
    let mut meta = Ext4Meta::new(size_bytes, Some(label));

    if let Some(uuid_str) = &part.uuid {
        if let Ok(uuid) = uuid_str.parse::<Uuid>() {
            meta.volume_id = *uuid.as_bytes();
        } else {
            return Err(FsError::Invalid("Invalid EXT4 UUID format. Expected UUID."));
        }
    }

    let mut formatter = Ext4Formatter::new(io, &meta);
    formatter.format(false)?;

    let mut allocator = Ext4Allocator::new(&meta);
    let mut injector = Ext4Injector::new(io, &mut allocator, &meta);
    injector.inject_tree(node)?;

    let mut checker = Ext4Checker::new(io, &meta);
    let report = checker.check_all()?;

    if report.has_error() {
        crate::log_normal!("{}", report.errors_only());
    }

    let mut parser = Ext4Resolver::new(io, &meta);
    let fs_root = parser.parse_tree("/*")?;
    let counts = fs_root.counts();

    crate::log_verbose!("On disk \n{}", fs_root);

    let dt = t0.elapsed().as_secs_f32();

    crate::log_info!(
        "\"{}\" formatted in {} and {} injected using RIM in {}s",
        part.name.bold(),
        "EXT4".magenta().bold(),
        counts.to_string().cyan(),
        format!("{dt:.2}").yellow()
    );

    Ok(())
}

fn format_raw(
    io: &mut dyn RimIO,
    entry: GptEntry,
    part: &Partition,
    base_dir: &Path,
) -> anyhow::Result<()> {
    let t0 = Instant::now();

    let start_lba = entry.start_lba;
    let end_lba = entry.end_lba;
    let offset = start_lba * SECTOR_SIZE;
    let size_bytes = (end_lba - start_lba + 1) * SECTOR_SIZE;
    let max_size = size_bytes as usize;

    io.set_offset(offset);

    if let Some(payload_relative) = &part.payload {
        let payload_path = base_dir.join(payload_relative);
        let mut file = std::fs::File::open(&payload_path).map_err(|e| {
            anyhow::anyhow!("Failed to open payload '{}': {}", payload_path.display(), e)
        })?;

        let metadata = file.metadata().map_err(|e| {
            anyhow::anyhow!(
                "Failed to get metadata for payload '{}': {}",
                payload_path.display(),
                e
            )
        })?;

        let file_size = metadata.len();
        if file_size > size_bytes {
            anyhow::bail!(
                "Payload '{}' is too large for partition '{}' ({} > {})",
                payload_path.display(),
                part.name,
                utils::pretty_bytes(file_size),
                utils::pretty_bytes(size_bytes)
            );
        }

        // Manual copy loop since RimIO doesn't implement std::io::Write
        let mut buf = [0u8; 65536]; // 64KB chunks
        let mut current_offset = 0;
        use std::io::Read;
        loop {
            let n = file
                .read(&mut buf)
                .map_err(|e| anyhow::anyhow!("Failed to read payload: {}", e))?;
            if n == 0 {
                break;
            }
            io.write_at(current_offset, &buf[..n])
                .map_err(|e| anyhow::anyhow!("Failed to write payload to partition: {}", e))?;
            current_offset += n as u64;
        }
    } else {
        io.zero_fill(0, max_size)
            .map_err(|e| anyhow::anyhow!("Failed to zero-fill partition: {}", e))?;
    }

    let dt = t0.elapsed().as_secs_f32();

    crate::log_info!(
        "\"{}\" formatted in {} {} using RIM in {}s",
        part.name.bold(),
        if part.payload.is_some() {
            "RAW (payload)".yellow()
        } else {
            "RAW (zeroed)".yellow()
        },
        if let Some(p) = &part.payload {
            format!("({})", p.display()).cyan()
        } else {
            "".clear()
        },
        format!("{dt:.2}").yellow()
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

fn parse_alignment_sectors(s: &str) -> anyhow::Result<u64> {
    let lower = s.trim().to_lowercase();
    let bytes = if let Some(stripped) = lower.strip_suffix("k") {
        stripped.trim().parse::<u64>()? * 1024
    } else if let Some(stripped) = lower.strip_suffix("m") {
        stripped.trim().parse::<u64>()? * 1024 * 1024
    } else if let Some(stripped) = lower.strip_suffix("g") {
        stripped.trim().parse::<u64>()? * 1024 * 1024 * 1024
    } else {
        // Assume bytes if no suffix
        s.trim().parse::<u64>()?
    };

    if bytes % SECTOR_SIZE != 0 {
        anyhow::bail!(
            "Alignment {} bytes is not a multiple of sector size ({})",
            bytes,
            SECTOR_SIZE
        );
    }

    Ok(bytes / SECTOR_SIZE)
}
