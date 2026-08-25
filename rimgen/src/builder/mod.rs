// SPDX-License-Identifier: MIT

pub mod gpt;
pub mod inject;
pub mod target;

use crate::errors::{GenError, GenResult};
use crate::layout::Layout;
use crate::layout::constants::*;
use gpt::{
    calculate_total_disk_sectors, parse_alignment_sectors, partition_to_gpt_partition_entry,
};
pub use inject::PartitionReport;
use rimimg::ImageFormat;
use rimio::prelude::*;
use std::path::Path;
use std::time::{Duration, Instant};
pub use target::DryRunMode;
pub(crate) use target::TargetImage;
use uuid::Uuid;

/// Events emitted during disk image generation.
#[derive(Debug, Clone)]
pub enum BuildEvent<'a> {
    LayoutPlanned {
        total_bytes: u64,
        total_sectors: u64,
    },
    GptWritten {
        duration: Duration,
    },
    PartitionStart {
        index: usize,
        total: usize,
        name: &'a str,
    },
    PartitionFormatted(&'a PartitionReport),
    PayloadProgress {
        current_bytes: u64,
        total_bytes: u64,
    },
}

/// Final summary report of a completed image build.
#[derive(Debug, Clone)]
pub struct BuildReport {
    pub total_bytes: u64,
    pub total_sectors: u64,
    pub gpt_duration: Duration,
    pub partitions: Vec<PartitionReport>,
    pub total_duration: Duration,
}

/// Handler function type for build events.
pub type BuildEventHandler = Box<dyn FnMut(BuildEvent)>;

/// Declarative builder for disk images.
pub struct ImageBuilder {
    pub layout: Layout,
    pub truncate: bool,
    pub dry_mode: DryRunMode,
    pub on_event: Option<BuildEventHandler>,
}

impl std::fmt::Debug for ImageBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageBuilder")
            .field("layout", &self.layout)
            .field("truncate", &self.truncate)
            .field("dry_mode", &self.dry_mode)
            .finish_non_exhaustive()
    }
}

impl ImageBuilder {
    /// Create a new builder with the given declarative layout.
    pub fn new(mut layout: Layout) -> Self {
        let _ = layout.resolve_partition();
        layout.assign_guids();
        Self {
            layout,
            truncate: false,
            dry_mode: DryRunMode::Off,
            on_event: None,
        }
    }

    /// Enable or disable GPT truncation.
    pub fn truncate(mut self, truncate: bool) -> Self {
        self.truncate = truncate;
        self
    }

    /// Set dry run mode.
    pub fn dry_mode(mut self, dry_mode: DryRunMode) -> Self {
        self.dry_mode = dry_mode;
        self
    }

    /// Set an event hook to receive real-time build notifications.
    pub fn on_event<F: FnMut(BuildEvent) + 'static>(mut self, callback: F) -> Self {
        self.on_event = Some(Box::new(callback));
        self
    }

    /// Build to an output file (automatically wrapped with rimimg if needed).
    pub fn build_to_file<P: AsRef<Path>>(&mut self, output: P) -> GenResult<BuildReport> {
        let mut noop = |_: BuildEvent| {};
        let cb: &mut dyn FnMut(BuildEvent) = match &mut self.on_event {
            Some(f) => f.as_mut(),
            None => &mut noop,
        };

        build_image_with_events(
            &self.layout,
            output.as_ref(),
            &self.truncate,
            self.dry_mode,
            cb,
        )
    }

    /// Build directly onto any target `RimIO` stream.
    pub fn build_to_io(&mut self, io: &mut dyn RimIO) -> GenResult<BuildReport> {
        let mut noop = |_: BuildEvent| {};
        let cb: &mut dyn FnMut(BuildEvent) = match &mut self.on_event {
            Some(f) => f.as_mut(),
            None => &mut noop,
        };

        build_on_io_with_events(&self.layout, io, cb)
    }
}

/// Build an image with automatic format wrapping via `rimimg`.
pub fn build_image(
    layout: &Layout,
    output: &Path,
    truncate: &bool,
    dry_mode: DryRunMode,
) -> GenResult<BuildReport> {
    build_image_with_events(layout, output, truncate, dry_mode, |_| {})
}

/// Build an image with automatic format wrapping and event callbacks.
pub fn build_image_with_events<F: FnMut(BuildEvent)>(
    layout: &Layout,
    output: &Path,
    truncate: &bool,
    dry_mode: DryRunMode,
    mut on_event: F,
) -> GenResult<BuildReport> {
    let format = ImageFormat::from_path(output)
        .map_err(|e| GenError::ContainerError(format!("Failed to determine format: {e}")))?;

    match format {
        ImageFormat::Raw => build_raw_with_events(layout, output, truncate, dry_mode, on_event),
        _ => {
            if matches!(dry_mode, DryRunMode::Plan) {
                return build_raw_with_events(layout, output, truncate, DryRunMode::Plan, on_event);
            }

            let temp_root = tempfile::tempdir()?;
            let temp_raw = temp_root.path().join("rim_temp.img");
            let rep = build_raw_with_events(layout, &temp_raw, truncate, dry_mode, &mut on_event)?;

            if matches!(dry_mode, DryRunMode::Off) {
                rimimg::wrap(&temp_raw, output, format).map_err(|e| {
                    GenError::ContainerError(format!("Failed to wrap container: {e}"))
                })?;
            }

            Ok(rep)
        }
    }
}

/// Create raw disk image on the filesystem.
pub fn build_raw(
    layout: &Layout,
    output: &Path,
    truncate: &bool,
    dry_mode: DryRunMode,
) -> GenResult<BuildReport> {
    build_raw_with_events(layout, output, truncate, dry_mode, |_| {})
}

/// Create raw disk image on the filesystem with event callbacks.
pub fn build_raw_with_events<F: FnMut(BuildEvent)>(
    layout: &Layout,
    output: &Path,
    truncate: &bool,
    dry_mode: DryRunMode,
    mut on_event: F,
) -> GenResult<BuildReport> {
    let t0 = Instant::now();
    let total_sectors = calculate_total_disk_sectors(layout);
    let total_bytes = total_sectors * DEFAULT_SECTOR_SIZE;

    // Determine alignment
    let align_sectors = if let Some(disk) = &layout.disk {
        if let Some(align_str) = &disk.alignment {
            parse_alignment_sectors(align_str)?
        } else {
            rimpart::gpt::align_lba_1m(DEFAULT_SECTOR_SIZE)
        }
    } else {
        rimpart::gpt::align_lba_1m(DEFAULT_SECTOR_SIZE)
    };

    let mut start = align_sectors;
    let mut partition_entries = vec![];

    for part in &layout.partitions {
        let sectors = gpt::size_to_sectors(&part.size);
        let end = start + sectors - 1;
        if end >= total_sectors {
            return Err(GenError::PartitionDoesNotFit {
                name: part.name.clone(),
                end_lba: end,
                total_sectors,
            });
        }
        partition_entries.push(partition_to_gpt_partition_entry(part, start, end)?);
        start = rimpart::gpt::align_up(end, align_sectors);
    }

    on_event(BuildEvent::LayoutPlanned {
        total_bytes,
        total_sectors,
    });

    // Plan mode => stop after plan
    if matches!(dry_mode, DryRunMode::Plan) {
        return Ok(BuildReport {
            total_bytes,
            total_sectors,
            gpt_duration: Duration::ZERO,
            partitions: vec![],
            total_duration: t0.elapsed(),
        });
    }

    // Open target
    let mut target = TargetImage::open(output, total_bytes, dry_mode)?;

    // Write MBR & GPT
    let gpt_t0 = Instant::now();
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

        rimpart::mbr::write_mbr_protective(&mut io, total_sectors)?;
        rimpart::gpt::write_gpt_from_entries(
            &mut io,
            &partition_entries,
            total_sectors,
            disk_guid,
        )?;

        if *truncate {
            let _ = rimpart::truncate_image_custom_sector(
                &mut io,
                &partition_entries,
                total_sectors,
                DEFAULT_SECTOR_SIZE,
            )?;
        }

        rimpart::validate_full_disk(&mut io)?;
    }
    let gpt_duration = gpt_t0.elapsed();
    on_event(BuildEvent::GptWritten {
        duration: gpt_duration,
    });

    // Format & inject
    let partitions = {
        let mut io = target.as_io()?;
        let (_hdr, entries) = rimpart::gpt::read_gpt_with_sector(&mut io, DEFAULT_SECTOR_SIZE)?;

        inject::format_inject_all(&mut io, layout, &entries, on_event)?
    };

    Ok(BuildReport {
        total_bytes,
        total_sectors,
        gpt_duration,
        partitions,
        total_duration: t0.elapsed(),
    })
}

/// Build a disk layout directly onto an open `RimIO` stream.
pub fn build_on_io(layout: &Layout, io: &mut dyn RimIO) -> GenResult<BuildReport> {
    build_on_io_with_events(layout, io, |_| {})
}

/// Build a disk layout directly onto an open `RimIO` stream with event callbacks.
pub fn build_on_io_with_events<F: FnMut(BuildEvent)>(
    layout: &Layout,
    io: &mut dyn RimIO,
    mut on_event: F,
) -> GenResult<BuildReport> {
    let t0 = Instant::now();
    let total_sectors = calculate_total_disk_sectors(layout);
    let total_bytes = total_sectors * DEFAULT_SECTOR_SIZE;

    let align_sectors = if let Some(disk) = &layout.disk {
        if let Some(align_str) = &disk.alignment {
            parse_alignment_sectors(align_str)?
        } else {
            rimpart::gpt::align_lba_1m(DEFAULT_SECTOR_SIZE)
        }
    } else {
        rimpart::gpt::align_lba_1m(DEFAULT_SECTOR_SIZE)
    };

    let mut start = align_sectors;
    let mut partition_entries = vec![];

    for part in &layout.partitions {
        let sectors = gpt::size_to_sectors(&part.size);
        let end = start + sectors - 1;
        if end >= total_sectors {
            return Err(GenError::PartitionDoesNotFit {
                name: part.name.clone(),
                end_lba: end,
                total_sectors,
            });
        }
        partition_entries.push(partition_to_gpt_partition_entry(part, start, end)?);
        start = rimpart::gpt::align_up(end, align_sectors);
    }

    on_event(BuildEvent::LayoutPlanned {
        total_bytes,
        total_sectors,
    });

    let disk_guid = if let Some(disk) = &layout.disk {
        if let Some(guid) = disk.guid {
            *guid.as_bytes()
        } else {
            *Uuid::new_v4().as_bytes()
        }
    } else {
        *Uuid::new_v4().as_bytes()
    };

    let gpt_t0 = Instant::now();
    rimpart::mbr::write_mbr_protective(io, total_sectors)?;
    rimpart::gpt::write_gpt_from_entries(io, &partition_entries, total_sectors, disk_guid)?;
    rimpart::validate_full_disk(io)?;
    let gpt_duration = gpt_t0.elapsed();

    on_event(BuildEvent::GptWritten {
        duration: gpt_duration,
    });

    let (_hdr, entries) = rimpart::gpt::read_gpt_with_sector(io, DEFAULT_SECTOR_SIZE)?;

    let partitions = inject::format_inject_all(io, layout, &entries, on_event)?;

    Ok(BuildReport {
        total_bytes,
        total_sectors,
        gpt_duration,
        partitions,
        total_duration: t0.elapsed(),
    })
}
