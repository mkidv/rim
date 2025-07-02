// rimgen-script/platform/mod.rs

use rimgen_layout::Layout;
use std::path::Path;
use rimgen_output::*;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux;


#[cfg(target_os = "macos")]
mod macos;

pub fn inject(layout: &Layout, img_path: &Path, debug: bool) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        use crate::platform::windows::WinScript;
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
        if debug {
            script = script.with_debug();
        }
        script.run(temp_root.path())?;

        if temp {
            vhd::unwrap_vhd_to_raw(&vhd_path, img_path)?;
        }
    }

    #[cfg(target_os = "linux")]
    {
        use crate::platform::linux::LinScript;
        let temp_root = tempfile::tempdir()?;
        let mut script = LinScript::new_from(layout, img_path)?;
        if debug {
            script = script.with_debug();
        }
        script.run(temp_root.path())?;
    }

    #[cfg(target_os = "macos")]
    {
        use crate::platform::macos::MacScript;
        let temp_root = tempfile::tempdir()?;
        let mut script = MacScript::new_from(layout, img_path)?;
        if debug {
            script = script.with_debug();
        }
        script.run(temp_root.path())?;
    }

    Ok(())
}
