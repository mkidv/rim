// SPDX-License-Identifier: MIT

//! `rimhost`: OS-native storage and tooling integration for RIM ecosystem.
//!
//! Provides integration with native host OS tools:
//! - Windows: PowerShell & Storage module (`Mount-VHD`, `Format-Volume`, `Dismount-VHD`)
//! - Linux: `losetup`, `kpartx`, `mkfs.*`, `mount`
//! - macOS: `hdiutil`, `diskutil`

#[macro_use]
mod macros;

pub mod cmd_builder;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

use cmd_builder::FormatCommandBuilder;
use rimgen::Layout;
use std::path::Path;

/// Execute host-native formatting and injection on an image file.
pub fn format_inject_host(layout: &Layout, img_path: &Path, dry_run: bool) -> anyhow::Result<()> {
    for p in &layout.partitions {
        if p.is_mountable() {
            p.fs.validate_binaries()?;
        }
    }

    #[cfg(target_os = "windows")]
    {
        use crate::windows::WinScript;

        if !dry_run {
            let is_vhd = img_path.extension().map(|e| e == "vhd").unwrap_or(false);
            let temp_root = tempfile::tempdir()?;

            let (vhd_path, temp) = if is_vhd {
                (img_path.to_path_buf(), false)
            } else {
                let temp_path = temp_root.path().join("temp.vhd");
                rimimg::vhd::wrap_raw_as_vhd_to(img_path, &temp_path)?;
                (temp_path, true)
            };

            let mut script = WinScript::new_from(layout, &vhd_path)?;
            script.run(temp_root.path())?;

            if temp {
                rimimg::vhd::unwrap_vhd_to_raw(&vhd_path, img_path)?;
            }
            return Ok(());
        }

        let script = WinScript::new_from(layout, img_path)?;
        script.dry_mode()?;
    }

    #[cfg(target_os = "linux")]
    {
        use crate::linux::LinScript;
        let mut script = LinScript::new_from(layout, img_path)?;

        if !dry_run {
            let temp_root = tempfile::tempdir()?;
            script.run(temp_root.path())?;
            return Ok(());
        }

        script.dry_mode()?;
    }

    #[cfg(target_os = "macos")]
    {
        use crate::macos::MacScript;
        let mut script = MacScript::new_from(layout, img_path)?;

        if !dry_run {
            let temp_root = tempfile::tempdir()?;
            script.run(temp_root.path())?;
            return Ok(());
        }

        script.dry_mode()?;
    }

    Ok(())
}
