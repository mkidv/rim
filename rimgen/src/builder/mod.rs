// SPDX-License-Identifier: MIT

pub mod gpt;
pub mod inject;

use crate::errors::{GenError, GenResult};
use crate::layout::constants::*;
use crate::layout::*;
#[cfg(feature = "std")]
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::time::Duration;
use gpt::{calculate_total_disk_sectors, partition_to_gpt_entry};
pub use inject::PartitionReport;
use rimio::prelude::*;
#[cfg(feature = "std")]
use std::time::Instant;

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

/// Build a layout directly onto an open `RimIO` stream.
pub fn build_on_io(layout: &mut Layout<'_>, io: &mut dyn RimIO) -> GenResult<BuildReport> {
    build_on_io_with_events(layout, io, |_| {})
}

/// Resolve a layout config with host files and build it onto an open `RimIO` stream.
#[cfg(feature = "std")]
pub fn build_config_on_io(layout: &LayoutConfig, io: &mut dyn RimIO) -> GenResult<BuildReport> {
    build_config_on_io_with_events(layout, io, |_| {})
}

/// Resolve a layout config with host files and build it onto an open `RimIO` stream.
#[cfg(feature = "std")]
pub fn build_config_on_io_with_events<F: for<'a> FnMut(BuildEvent<'a>)>(
    layout: &LayoutConfig,
    io: &mut dyn RimIO,
    on_event: F,
) -> GenResult<BuildReport> {
    let mut resolved = layout.to_layout(&mut crate::guid::RandomGuidGenerator)?;
    for (i, part) in layout.partitions.iter().enumerate() {
        let mountpoint = part.mountpoint.as_deref().unwrap_or("");
        if !mountpoint.is_empty() {
            resolved.partitions[i].source_mountpoint = Some(layout.base_dir.join(mountpoint));
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
    build_on_io_with_events(&mut resolved, io, on_event)
}

/// Build a layout directly onto an open `RimIO` stream with event callbacks.
pub fn build_on_io_with_events<F: for<'a> FnMut(BuildEvent<'a>)>(
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
