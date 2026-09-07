// SPDX-License-Identifier: MIT

pub mod constants;
pub mod error;
pub mod filesystem;
pub mod partition;
pub mod size;

pub use constants::*;
pub use error::*;
pub use filesystem::*;
pub use partition::*;
pub use size::*;

use crate::errors::GenResult;
use crate::guid::GuidGenerator;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use rimfs::core::resolver::FsNode;
use rimio::prelude::RimRead;
use serde::Deserialize;
#[cfg(feature = "std")]
use std::fs;
#[cfg(feature = "std")]
use std::path::{Path, PathBuf};

pub struct Partition<'a> {
    pub name: String,
    pub kind: PartitionKind,
    pub size_sectors: u64,
    pub fs: Filesystem,
    pub bootable: bool,
    pub guid: [u8; 16],
    pub label: Option<String>,
    pub uuid: Option<String>,
    pub root: Option<FsNode<'a>>,
    #[cfg(feature = "std")]
    pub source_mountpoint: Option<PathBuf>,
    pub raw_source: Option<Box<dyn RimRead + 'a>>,
    pub raw_size: u64,
}

impl<'a> core::fmt::Debug for Partition<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut debug = f.debug_struct("Partition");
        debug
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("size_sectors", &self.size_sectors)
            .field("fs", &self.fs)
            .field("bootable", &self.bootable)
            .field("guid", &self.guid)
            .field("label", &self.label)
            .field("uuid", &self.uuid)
            .field("root", &self.root);
        #[cfg(feature = "std")]
        debug.field("source_mountpoint", &self.source_mountpoint);
        debug
            .field(
                "raw_source",
                &self.raw_source.as_ref().map(|_| "<dyn RimRead>"),
            )
            .field("raw_size", &self.raw_size)
            .finish()
    }
}

impl<'a> Partition<'a> {
    pub fn new(
        name: impl Into<String>,
        kind: PartitionKind,
        fs: Filesystem,
        size_sectors: u64,
        guid: [u8; 16],
    ) -> Self {
        Self {
            name: name.into(),
            kind,
            size_sectors,
            fs,
            bootable: false,
            guid,
            label: None,
            uuid: None,
            root: None,
            #[cfg(feature = "std")]
            source_mountpoint: None,
            raw_source: None,
            raw_size: 0,
        }
    }

    pub fn with_root(mut self, root: FsNode<'a>) -> Self {
        self.root = Some(root);
        self
    }

    pub fn with_bootable(mut self, bootable: bool) -> Self {
        self.bootable = bootable;
        self
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_uuid(mut self, uuid: impl Into<String>) -> Self {
        self.uuid = Some(uuid.into());
        self
    }

    pub fn with_raw_source(mut self, source: Box<dyn RimRead + 'a>, size: u64) -> Self {
        self.raw_source = Some(source);
        self.raw_size = size;
        self
    }
}

#[derive(Debug)]
pub struct Layout<'a> {
    pub disk_guid: [u8; 16],
    pub alignment_sectors: u64,
    pub partitions: Vec<Partition<'a>>,
    pub total_disk_sectors: Option<u64>,
}

impl<'a> Layout<'a> {
    pub fn new(disk_guid: [u8; 16]) -> Self {
        Self {
            disk_guid,
            alignment_sectors: rimpart::gpt::align_lba_1m(DEFAULT_SECTOR_SIZE),
            partitions: Vec::new(),
            total_disk_sectors: None,
        }
    }

    pub fn with_alignment_sectors(mut self, align: u64) -> Self {
        self.alignment_sectors = align;
        self
    }

    pub fn with_total_disk_sectors(mut self, total: u64) -> Self {
        self.total_disk_sectors = Some(total);
        self
    }

    pub fn add_partition(mut self, partition: Partition<'a>) -> Self {
        self.partitions.push(partition);
        self
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct LayoutConfig {
    #[cfg(feature = "std")]
    #[serde(skip)]
    pub base_dir: PathBuf,
    pub partitions: Vec<PartitionConfig>,
    pub disk: Option<DiskConfig>,
}

impl LayoutConfig {
    #[cfg(feature = "std")]
    pub fn from_file(path: &Path) -> GenResult<Self> {
        let content = fs::read_to_string(path)?;
        let mut layout: LayoutConfig = toml::from_str(&content)?;
        layout.base_dir = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .canonicalize()?;
        layout.resolve_partition()?;
        layout.assign_guids();
        Ok(layout)
    }

    #[cfg(feature = "std")]
    pub fn from_str(toml_str: &str, base_dir: Option<PathBuf>) -> GenResult<Self> {
        let mut layout: LayoutConfig = toml::from_str(toml_str)?;
        layout.base_dir = base_dir.unwrap_or_else(|| PathBuf::from("."));
        layout.resolve_partition()?;
        layout.assign_guids();
        Ok(layout)
    }

    #[cfg(feature = "std")]
    pub fn resolve_partition(&mut self) -> GenResult<()> {
        let mut auto_indices = Vec::new();
        let mut fixed_sum_mb: u64 = 0;

        for (i, part) in self.partitions.iter().enumerate() {
            match part.size {
                Size::Auto => auto_indices.push(i),
                Size::Fixed(mb) => fixed_sum_mb = fixed_sum_mb.saturating_add(mb),
            }
        }

        let disk_size_mb = self.disk.as_ref().and_then(|d| match d.size {
            Some(Size::Fixed(mb)) => Some(mb),
            _ => None,
        });

        if let Some(total_mb) = disk_size_mb {
            let overhead_mb = match self.effective_partition_table() {
                crate::builder::PartitionTable::Gpt => 2,
                crate::builder::PartitionTable::None => 0,
            };

            let available_mb = total_mb.saturating_sub(overhead_mb);
            if available_mb < fixed_sum_mb {
                return Err(LayoutError::InvalidConfig(
                    "Partitions total size exceeds [disk] size",
                )
                .into());
            }

            let mut remaining_mb = available_mb - fixed_sum_mb;

            for (idx_in_auto, &part_idx) in auto_indices.iter().enumerate() {
                let is_last_auto = idx_in_auto == auto_indices.len() - 1;
                let part = &mut self.partitions[part_idx];

                let source_path = if let Some(mp) = &part.mountpoint {
                    Some(self.base_dir.join(mp))
                } else if let Some(payload) = &part.payload {
                    Some(self.base_dir.join(payload))
                } else {
                    None
                };
                let needed_bytes = if let Some(path) = &source_path {
                    if path.exists() {
                        calculate_needed_bytes(path)?
                    } else {
                        0
                    }
                } else {
                    0
                };

                let needed_mb = if needed_bytes > 0 {
                    ((needed_bytes as f64 * 1.1) / (1024.0 * 1024.0)).ceil() as u64
                } else {
                    0
                };

                if is_last_auto {
                    if remaining_mb < needed_mb {
                        return Err(LayoutError::InvalidConfig(
                            "Remaining [disk] space is smaller than files in auto partition",
                        )
                        .into());
                    }
                    part.size = Size::Fixed(remaining_mb);
                } else {
                    let allocated_mb = if needed_mb > 0 {
                        needed_mb.min(remaining_mb)
                    } else {
                        DEFAULT_AUTO_SIZE_MB.min(remaining_mb)
                    };
                    remaining_mb = remaining_mb.saturating_sub(allocated_mb);
                    part.size = Size::Fixed(allocated_mb);
                }
            }
        } else {
            for part in &mut self.partitions {
                if let Size::Auto = part.size {
                    let source_path = self.base_dir.join(part.mountpoint.as_deref().unwrap_or(""));
                    let size_bytes = calculate_needed_bytes(&source_path)?;
                    let size_mb = ((size_bytes as f64 * 1.1) / (1024.0 * 1024.0))
                        .ceil()
                        .max(DEFAULT_AUTO_SIZE_MB as f64) as u64;
                    part.size = Size::Fixed(size_mb);
                }
            }
        }

        for part in &mut self.partitions {
            if part.kind.is_none() {
                part.kind = Some(part.effective_kind());
            }
        }
        Ok(())
    }

    #[cfg(feature = "std")]
    pub fn assign_guids(&mut self) {
        for part in &mut self.partitions {
            if part.guid.is_none() {
                part.guid = Some(uuid::Uuid::new_v4());
            }
        }
    }

    pub fn effective_partition_table(&self) -> crate::builder::PartitionTable {
        self.disk
            .as_ref()
            .map(|d| d.effective_partition_table())
            .unwrap_or(crate::builder::PartitionTable::Gpt)
    }

    pub fn validate(&self) -> GenResult<()> {
        if let Some(disk) = &self.disk {
            disk.validate()?;
        }
        for p in &self.partitions {
            p.validate()?;
        }
        Ok(())
    }

    pub fn to_layout<G: GuidGenerator>(&self, generator: &mut G) -> GenResult<Layout<'static>> {
        let disk_guid = if let Some(disk) = &self.disk {
            if let Some(guid) = disk.guid {
                *guid.as_bytes()
            } else {
                generator.generate_guid()
            }
        } else {
            generator.generate_guid()
        };

        let alignment_sectors = if let Some(disk) = &self.disk {
            if let Some(align_str) = &disk.alignment {
                crate::builder::gpt::parse_alignment_sectors(align_str)?
            } else {
                rimpart::gpt::align_lba_1m(DEFAULT_SECTOR_SIZE)
            }
        } else {
            rimpart::gpt::align_lba_1m(DEFAULT_SECTOR_SIZE)
        };

        let mut resolved_parts = Vec::with_capacity(self.partitions.len());
        for part in &self.partitions {
            let sectors = crate::builder::gpt::size_to_sectors(&part.size);
            let guid = if let Some(g) = part.guid {
                *g.as_bytes()
            } else {
                generator.generate_guid()
            };

            resolved_parts.push(Partition {
                name: part.name.clone(),
                kind: part.effective_kind(),
                size_sectors: sectors,
                fs: part.fs,
                bootable: part.bootable,
                guid,
                label: part.label.clone(),
                uuid: part.uuid.clone(),
                root: None,
                #[cfg(feature = "std")]
                source_mountpoint: None,
                raw_source: None,
                raw_size: 0,
            });
        }

        let total_disk_sectors = if let Some(disk) = &self.disk {
            if let Some(Size::Fixed(mb)) = disk.size {
                Some((mb * 1024 * 1024) / DEFAULT_SECTOR_SIZE)
            } else {
                None
            }
        } else {
            None
        };

        Ok(Layout {
            disk_guid,
            alignment_sectors,
            partitions: resolved_parts,
            total_disk_sectors,
        })
    }
}

impl core::fmt::Display for LayoutConfig {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if let Some(disk) = &self.disk {
            writeln!(f, "Disk Configuration:")?;
            if let Some(size) = &disk.size {
                writeln!(f, "  Size: {size}")?;
            }
            if let Some(table) = &disk.table {
                writeln!(f, "  Partition Table: {table}")?;
            }
            if let Some(align) = &disk.alignment {
                writeln!(f, "  Alignment: {align}")?;
            }
            if let Some(guid) = &disk.guid {
                writeln!(f, "  Disk GUID: {guid}")?;
            }
            writeln!(f)?;
        }

        writeln!(
            f,
            "\n  ┌────┬──────────────────────────────┬────────┬────────────┬─────────┬──────┐"
        )?;
        writeln!(
            f,
            "  | Id | Name                         | Kind   | Size       | FS      | Boot |"
        )?;
        writeln!(
            f,
            "  ├────┼──────────────────────────────┼────────┼────────────┼─────────┼──────┤"
        )?;
        for (i, p) in self.partitions.iter().enumerate() {
            let kind = p.effective_kind();
            let size = match p.size {
                Size::Fixed(mb) => alloc::format!("{mb} MB"),
                Size::Auto => "auto".into(),
            };
            writeln!(
                f,
                "  | {i:<2} | {n:<28} | {k:<6} | {s:>10} | {fs:>7} | {b:>4} |",
                n = if p.name.len() > 28 {
                    &p.name[..28]
                } else {
                    &p.name
                },
                k = alloc::format!("{kind:?}"),
                s = size,
                fs = alloc::format!("{}", p.fs),
                b = if p.bootable { "yes" } else { "no" },
            )?;
        }
        writeln!(
            f,
            "  └────┴──────────────────────────────┴────────┴────────────┴─────────┴──────┘"
        )
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct DiskConfig {
    #[serde(default, alias = "align")]
    pub alignment: Option<String>,
    #[serde(default, alias = "disk_guid", alias = "disk-guid")]
    pub guid: Option<uuid::Uuid>,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(
        default,
        alias = "partition_table",
        alias = "partition-table",
        alias = "type"
    )]
    pub table: Option<String>,
}

impl DiskConfig {
    pub fn effective_partition_table(&self) -> crate::builder::PartitionTable {
        if let Some(table) = &self.table {
            let trimmed = table.trim().to_lowercase();
            if trimmed == "none" || trimmed == "raw" {
                return crate::builder::PartitionTable::None;
            }
        }
        crate::builder::PartitionTable::Gpt
    }

    pub fn validate(&self) -> GenResult<()> {
        if let Some(table) = &self.table {
            let t = table.trim().to_lowercase();
            if t != "gpt" && t != "none" && t != "raw" {
                return Err(LayoutError::InvalidConfig(
                    "Unsupported partition table in [disk]. Supported values: 'gpt', 'none', 'raw'",
                )
                .into());
            }
        }
        if let Some(align) = &self.alignment {
            crate::builder::gpt::parse_alignment_sectors(align)?;
        }
        Ok(())
    }
}
