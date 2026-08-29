// SPDX-License-Identifier: MIT

pub mod gpt;
pub mod inject;
#[cfg(feature = "std")]
pub mod target;

use crate::errors::{GenError, GenResult};
use crate::layout::constants::*;
use crate::layout::*;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::time::Duration;
use gpt::{calculate_total_disk_sectors, partition_to_gpt_entry};
#[cfg(feature = "std")]
use gpt::{
    calculate_total_disk_sectors_from_config, parse_alignment_sectors,
    partition_config_to_gpt_entry,
};
pub use inject::PartitionReport;
#[cfg(feature = "std")]
use rimfs::core::resolver::FsTreeResolver;
#[cfg(feature = "std")]
use rimimg::ImageFormat;
use rimio::prelude::*;
#[cfg(feature = "std")]
use std::path::Path;
#[cfg(feature = "std")]
use std::time::Instant;
#[cfg(feature = "std")]
pub(crate) use target::TargetImage;
#[cfg(feature = "std")]
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DryRunMode {
    Off,
    Plan,
    Tempfile,
}

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
#[cfg(feature = "std")]
pub struct ImageBuilder {
    pub layout: LayoutConfig,
    pub truncate: bool,
    pub dry_mode: DryRunMode,
    pub on_event: Option<BuildEventHandler>,
}

#[cfg(feature = "std")]
impl core::fmt::Debug for ImageBuilder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ImageBuilder")
            .field("layout", &self.layout)
            .field("truncate", &self.truncate)
            .field("dry_mode", &self.dry_mode)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "std")]
impl ImageBuilder {
    /// Create a new builder with the given declarative layout config.
    pub fn new(mut layout: LayoutConfig) -> Self {
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

        build_layout_on_io(&self.layout, io, cb)
    }
}

/// Build an image with automatic format wrapping via `rimimg`.
#[cfg(feature = "std")]
pub fn build_image(
    layout: &LayoutConfig,
    output: &Path,
    truncate: &bool,
    dry_mode: DryRunMode,
) -> GenResult<BuildReport> {
    build_image_with_events(layout, output, truncate, dry_mode, |_| {})
}

/// Build an image with automatic format wrapping and event callbacks.
#[cfg(feature = "std")]
pub fn build_image_with_events<F: FnMut(BuildEvent)>(
    layout: &LayoutConfig,
    output: &Path,
    truncate: &bool,
    dry_mode: DryRunMode,
    mut on_event: F,
) -> GenResult<BuildReport> {
    let format = ImageFormat::from_path(output)
        .map_err(|e| GenError::ContainerError(alloc::format!("Failed to determine format: {e}")))?;

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
                    GenError::ContainerError(alloc::format!("Failed to wrap container: {e}"))
                })?;
            }

            Ok(rep)
        }
    }
}

/// Create raw disk image on the filesystem.
#[cfg(feature = "std")]
pub fn build_raw(
    layout: &LayoutConfig,
    output: &Path,
    truncate: &bool,
    dry_mode: DryRunMode,
) -> GenResult<BuildReport> {
    build_raw_with_events(layout, output, truncate, dry_mode, |_| {})
}

/// Create raw disk image on the filesystem with event callbacks.
#[cfg(feature = "std")]
pub fn build_raw_with_events<F: FnMut(BuildEvent)>(
    layout: &LayoutConfig,
    output: &Path,
    truncate: &bool,
    dry_mode: DryRunMode,
    mut on_event: F,
) -> GenResult<BuildReport> {
    let t0 = Instant::now();
    let total_sectors = calculate_total_disk_sectors_from_config(layout);
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
        partition_entries.push(partition_config_to_gpt_entry(part, start, end)?);
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

        let mut resolved = layout.to_layout(&mut crate::guid::RandomGuidGenerator)?;
        let mut parser = rimfs::core::StdResolver::new();
        for (i, part) in layout.partitions.iter().enumerate() {
            let mountpoint = part.mountpoint.as_deref().unwrap_or("");
            if !mountpoint.is_empty() {
                let source_path = layout.base_dir.join(mountpoint);
                let node = parser.resolve_tree(source_path.to_str().unwrap_or(""))?;
                resolved.partitions[i].root = Some(node);
            }
            if let Some(payload_relative) = &part.payload {
                let payload_path = layout.base_dir.join(payload_relative);
                let file = std::fs::File::open(&payload_path)?;
                let file_size = file.metadata()?.len();
                let file_io = rimio::prelude::ReadOnlyFileRimIO::from_file(file)?;
                resolved.partitions[i].raw_source = Some(Box::new(file_io));
                resolved.partitions[i].raw_size = file_size;
            }
        }

        inject::format_inject_resolved_all(&mut io, &mut resolved, &entries, on_event)?
    };

    Ok(BuildReport {
        total_bytes,
        total_sectors,
        gpt_duration,
        partitions,
        total_duration: t0.elapsed(),
    })
}

/// Build a layout directly onto an open `RimIO` stream using host std files.
#[cfg(feature = "std")]
pub fn build_layout_on_io<F: FnMut(BuildEvent)>(
    layout: &LayoutConfig,
    io: &mut dyn RimIO,
    on_event: F,
) -> GenResult<BuildReport> {
    let mut resolved = layout.to_layout(&mut crate::guid::RandomGuidGenerator)?;
    let mut parser = rimfs::core::StdResolver::new();
    for (i, part) in layout.partitions.iter().enumerate() {
        let mountpoint = part.mountpoint.as_deref().unwrap_or("");
        if !mountpoint.is_empty() {
            let source_path = layout.base_dir.join(mountpoint);
            let node = parser.resolve_tree(source_path.to_str().unwrap_or(""))?;
            resolved.partitions[i].root = Some(node);
        }
        if let Some(payload_relative) = &part.payload {
            let payload_path = layout.base_dir.join(payload_relative);
            let file = std::fs::File::open(&payload_path)?;
            let file_size = file.metadata()?.len();
            let file_io = rimio::prelude::ReadOnlyFileRimIO::from_file(file)?;
            resolved.partitions[i].raw_source = Some(Box::new(file_io));
            resolved.partitions[i].raw_size = file_size;
        }
    }
    build_on_io(&mut resolved, io, on_event)
}

/// Build a layout directly onto an open `RimIO` stream.
pub fn build_on_io_simple(layout: &mut Layout<'_>, io: &mut dyn RimIO) -> GenResult<BuildReport> {
    build_on_io(layout, io, |_| {})
}

/// Build a layout directly onto an open `RimIO` stream with event callbacks.
pub fn build_on_io<F: FnMut(BuildEvent)>(
    layout: &mut Layout<'_>,
    io: &mut dyn RimIO,
    mut on_event: F,
) -> GenResult<BuildReport> {
    #[cfg(feature = "std")]
    let t0 = Instant::now();
    let total_sectors = calculate_total_disk_sectors(layout);
    let total_bytes = total_sectors * DEFAULT_SECTOR_SIZE;

    let align_sectors = layout.alignment_sectors;
    let mut start = align_sectors;
    let mut partition_entries = Vec::with_capacity(layout.partitions.len());

    for part in &layout.partitions {
        let sectors = part.size_sectors;
        let end = start + sectors - 1;
        if end >= total_sectors {
            return Err(GenError::PartitionDoesNotFit {
                name: part.name.clone(),
                end_lba: end,
                total_sectors,
            });
        }
        partition_entries.push(partition_to_gpt_entry(part, start, end));
        start = rimpart::gpt::align_up(end, align_sectors);
    }

    on_event(BuildEvent::LayoutPlanned {
        total_bytes,
        total_sectors,
    });

    #[cfg(feature = "std")]
    let gpt_t0 = Instant::now();
    rimpart::mbr::write_mbr_protective(io, total_sectors)?;
    rimpart::gpt::write_gpt_from_entries(io, &partition_entries, total_sectors, layout.disk_guid)?;
    rimpart::validate_full_disk(io)?;
    #[cfg(feature = "std")]
    let gpt_duration = gpt_t0.elapsed();
    #[cfg(not(feature = "std"))]
    let gpt_duration = Duration::ZERO;

    on_event(BuildEvent::GptWritten {
        duration: gpt_duration,
    });

    let (_hdr, entries) = rimpart::gpt::read_gpt_with_sector(io, DEFAULT_SECTOR_SIZE)?;

    let partitions = inject::format_inject_resolved_all(io, layout, &entries, on_event)?;
    io.set_offset(0);

    #[cfg(feature = "std")]
    let total_duration = t0.elapsed();
    #[cfg(not(feature = "std"))]
    let total_duration = Duration::ZERO;

    Ok(BuildReport {
        total_bytes,
        total_sectors,
        gpt_duration,
        partitions,
        total_duration,
    })
}
