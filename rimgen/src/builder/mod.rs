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

/// Partition table mode used while generating an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionTable {
    /// Write a protective MBR and GPT partition table.
    Gpt,
    /// Do not write GPT or MBR structures; volumes are laid out directly.
    None,
}

/// Options controlling how a layout is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildOptions {
    pub partition_table: PartitionTable,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            partition_table: PartitionTable::Gpt,
        }
    }
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

/// Build a layout directly onto an open `RimIO` stream.
pub fn build_on_io(layout: &mut Layout<'_>, io: &mut dyn RimIO) -> GenResult<BuildReport> {
    build_on_io_with_options_and_events(layout, io, BuildOptions::default(), |_| {})
}

/// Build a layout directly onto an open `RimIO` stream with custom options.
pub fn build_on_io_with_options(
    layout: &mut Layout<'_>,
    io: &mut dyn RimIO,
    options: BuildOptions,
) -> GenResult<BuildReport> {
    build_on_io_with_options_and_events(layout, io, options, |_| {})
}

/// Resolve a layout config with host files and build it onto an open `RimIO` stream.
#[cfg(feature = "std")]
pub fn build_config_on_io(layout: &LayoutConfig, io: &mut dyn RimIO) -> GenResult<BuildReport> {
    build_config_on_io_with_options_and_events(layout, io, BuildOptions::default(), |_| {})
}

/// Resolve a layout config with host files and build it onto an open `RimIO` stream.
#[cfg(feature = "std")]
pub fn build_config_on_io_with_options(
    layout: &LayoutConfig,
    io: &mut dyn RimIO,
    options: BuildOptions,
) -> GenResult<BuildReport> {
    build_config_on_io_with_options_and_events(layout, io, options, |_| {})
}

/// Resolve a layout config with host files and build it onto an open `RimIO` stream.
#[cfg(feature = "std")]
pub fn build_config_on_io_with_events<F: for<'a> FnMut(BuildEvent<'a>)>(
    layout: &LayoutConfig,
    io: &mut dyn RimIO,
    on_event: F,
) -> GenResult<BuildReport> {
    build_config_on_io_with_options_and_events(layout, io, BuildOptions::default(), on_event)
}

/// Resolve a layout config with host files and build it onto an open `RimIO` stream.
#[cfg(feature = "std")]
pub fn build_config_on_io_with_options_and_events<F: for<'a> FnMut(BuildEvent<'a>)>(
    layout: &LayoutConfig,
    io: &mut dyn RimIO,
    options: BuildOptions,
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
    build_on_io_with_options_and_events(&mut resolved, io, options, on_event)
}

/// Build a layout directly onto an open `RimIO` stream with event callbacks.
pub fn build_on_io_with_events<F: for<'a> FnMut(BuildEvent<'a>)>(
    layout: &mut Layout<'_>,
    io: &mut dyn RimIO,
    on_event: F,
) -> GenResult<BuildReport> {
    build_on_io_with_options_and_events(layout, io, BuildOptions::default(), on_event)
}

/// Build a layout directly onto an open `RimIO` stream with custom options and event callbacks.
pub fn build_on_io_with_options_and_events<F: for<'a> FnMut(BuildEvent<'a>)>(
    layout: &mut Layout<'_>,
    io: &mut dyn RimIO,
    options: BuildOptions,
    mut on_event: F,
) -> GenResult<BuildReport> {
    #[cfg(feature = "std")]
    let t0 = Instant::now();
    let total_sectors = calculate_total_disk_sectors_with_options(layout, options);
    let total_bytes = total_sectors * DEFAULT_SECTOR_SIZE;

    let first_lba = match options.partition_table {
        PartitionTable::Gpt => layout.alignment_sectors,
        PartitionTable::None => 0,
    };
    let partition_entries = plan_partition_entries(layout, first_lba, total_sectors)?;

    on_event(BuildEvent::LayoutPlanned {
        total_bytes,
        total_sectors,
    });

    #[cfg(feature = "std")]
    let gpt_t0 = Instant::now();
    if options.partition_table == PartitionTable::Gpt {
        rimpart::mbr::write_mbr_protective(io, total_sectors)?;
        rimpart::gpt::write_gpt_from_entries(
            io,
            &partition_entries,
            total_sectors,
            layout.disk_guid,
        )?;
        rimpart::validate_full_disk(io)?;
    }
    #[cfg(feature = "std")]
    let gpt_duration = gpt_t0.elapsed();
    #[cfg(not(feature = "std"))]
    let gpt_duration = Duration::ZERO;

    if options.partition_table == PartitionTable::Gpt {
        on_event(BuildEvent::GptWritten {
            duration: gpt_duration,
        });
    }

    let partitions = inject::format_inject_resolved_all(io, layout, &partition_entries, on_event)?;
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

/// Calculate total disk sectors needed for a layout and build options.
pub fn calculate_total_disk_sectors_with_options(
    layout: &Layout<'_>,
    options: BuildOptions,
) -> u64 {
    match options.partition_table {
        PartitionTable::Gpt => calculate_total_disk_sectors(layout),
        PartitionTable::None => calculate_total_content_sectors(layout),
    }
}

/// Calculate total disk sectors needed for a layout config and build options.
#[cfg(feature = "std")]
pub fn calculate_total_disk_sectors_from_config_with_options(
    layout: &LayoutConfig,
    options: BuildOptions,
) -> GenResult<u64> {
    if options.partition_table == PartitionTable::Gpt {
        return Ok(gpt::calculate_total_disk_sectors_from_config(layout));
    }

    let resolved = layout.to_layout(&mut crate::guid::RandomGuidGenerator)?;
    Ok(calculate_total_disk_sectors_with_options(
        &resolved, options,
    ))
}

fn calculate_total_content_sectors(layout: &Layout<'_>) -> u64 {
    if layout.partitions.is_empty() {
        return 0;
    }

    let mut start = 0;
    let mut last_end = 0;
    for part in &layout.partitions {
        last_end = start + part.size_sectors - 1;
        start = rimpart::gpt::align_up(last_end, layout.alignment_sectors);
    }

    last_end + 1
}

fn plan_partition_entries(
    layout: &Layout<'_>,
    first_lba: u64,
    total_sectors: u64,
) -> GenResult<Vec<rimpart::gpt::GptEntry>> {
    let align_sectors = layout.alignment_sectors;
    let mut start = first_lba;
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

    Ok(partition_entries)
}
