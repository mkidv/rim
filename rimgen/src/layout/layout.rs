// SPDX-License-Identifier: MIT

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{layout::partition::*, layout::size::*};

pub const DEFAULT_AUTO_SIZE_MB: u64 = 64;

#[derive(Debug, Deserialize)]
pub struct Layout {
    #[serde(skip)]
    pub base_dir: PathBuf,
    pub partitions: Vec<Partition>,
}

impl Layout {
    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        let mut layout: Layout = toml::from_str(&content)?;
        layout.base_dir = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .canonicalize()?
            .to_path_buf();
        layout.resolve_partition()?;
        layout.assign_guids();
        Ok(layout)
    }

    pub fn resolve_partition(&mut self) -> anyhow::Result<()> {
        for part in &mut self.partitions {
            if let Size::Auto = part.size {
                let source_path = self.base_dir.join(&part.mountpoint.as_deref().unwrap_or(""));
                let size_bytes = calculate_needed_bytes(&source_path)?;
                let size_mb = ((size_bytes as f64 * 1.1) / (1024.0 * 1024.0))
                    .ceil()
                    .max(DEFAULT_AUTO_SIZE_MB as f64) as u64;
                crate::log_verbose!("Auto-size calculated for '{}': {} MB", part.name, size_mb);
                part.size = Size::Fixed(size_mb);
            }
            if part.kind.is_none() {
                part.kind = Some(part.effective_kind());
            }
        }
        Ok(())
    }

    pub fn assign_guids(&mut self) {
        for part in &mut self.partitions {
            if part.guid.is_none() {
                part.guid = Some(uuid::Uuid::new_v4());
            }
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        self.partitions.iter().try_for_each(|p| p.validate())?;

        Ok(())
    }
}

impl core::fmt::Display for Layout {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
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
                Size::Fixed(mb) => format!("{mb} MB"),
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
                k = format!("{kind:?}"),
                s = size,
                fs = format!("{}", p.fs),
                b = if p.bootable { "yes" } else { "no" },
            )?;
        }
        writeln!(
            f,
            "  └────┴──────────────────────────────┴────────┴────────────┴─────────┴──────┘"
        )
    }
}
