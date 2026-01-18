// SPDX-License-Identifier: MIT

use crate::host::cmd_builder::FormatCommandBuilder;
use crate::layout::Layout;
use crate::out::target::DryRunMode;
use crate::out::*;
use std::path::Path;

mod cmd_builder;

#[macro_use]
mod macros;

#[cfg(feature = "host-scripts")]
#[cfg(target_os = "windows")]
mod windows;

#[cfg(feature = "host-scripts")]
#[cfg(target_os = "linux")]
mod linux;

#[cfg(feature = "host-scripts")]
#[cfg(target_os = "macos")]
mod macos;

#[cfg(feature = "host-scripts")]
pub fn format_inject_host(
    layout: &Layout,
    img_path: &Path,
    dry_mode: DryRunMode,
) -> anyhow::Result<()> {
    for p in &layout.partitions {
        if p.is_mountable() {
            p.fs.validate_binaries()?;
        }
    }

    #[cfg(target_os = "windows")]
    {
        use crate::host::windows::WinScript;

        if matches!(dry_mode, DryRunMode::Off) {
            let is_vhd = img_path.extension().map(|e| e == "vhd").unwrap_or(false);
            let temp_root = tempfile::tempdir()?;

            let (vhd_path, temp) = if is_vhd {
                (img_path.to_path_buf(), false)
            } else {
                let temp_path = temp_root.path().join("temp.vhd");
                vhd::wrap_raw_as_vhd_to(img_path, &temp_path)?;
                (temp_path, true)
            };

            let mut script = WinScript::new_from(layout, &vhd_path)?;

            script.run(temp_root.path())?;

            if temp {
                vhd::unwrap_vhd_to_raw(&vhd_path, img_path)?;
            }
            return Ok(());
        }

        crate::log_info!("Dry-run - printing host-script.");
        let script = WinScript::new_from(layout, img_path)?;
        script.dry_mode()?;
    }

    #[cfg(target_os = "linux")]
    {
        use crate::host::linux::LinScript;
        let mut script = LinScript::new_from(layout, img_path)?;

        if matches!(dry_mode, DryRunMode::Off) {
            let temp_root = tempfile::tempdir()?;
            script.run(temp_root.path())?;
            return Ok(());
        }

        crate::log_info!("Dry-run - printing host-script.");
        script.dry_mode()?;
    }

    #[cfg(target_os = "macos")]
    {
        use crate::host::macos::MacScript;
        let mut script = MacScript::new_from(layout, &img_path)?;

        if matches!(dry_mode, DryRunMode::Off) {
            let temp_root = tempfile::tempdir()?;
            script.run(temp_root.path())?;
            return Ok(());
        }

        crate::log_info!("Dry-run - printing host-script.");
        script.dry_mode()?;
    }

    Ok(())
}
