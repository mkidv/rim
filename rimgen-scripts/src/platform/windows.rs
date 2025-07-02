// rimgen/platform/windows/win_script.rs

use rimgen_layout::*;
use rimgen_output::*;
use std::path::Path;
use std::process::Command;
use std::{fs::File, io::Write};

use crate::cmd_builder::FormatCommandBuilder;

pub struct WinScript {
    lines: Vec<String>,
    debug: bool,
}

impl WinScript {
    fn new() -> Self {
        Self {
            lines: vec![],
            debug: false,
        }
    }

    pub fn with_debug(mut self) -> Self {
        self.debug = true;
        self
    }

    fn add<S: Into<String>>(&mut self, line: S) -> &mut Self {
        self.lines.push(line.into());
        self
    }

    pub fn new_from(layout: &Layout, vhd_path: &Path) -> anyhow::Result<Self> {
        let mut ps = WinScript::new();

        ps.add("try {");
        ps.add(format!(
            "$disk = Mount-VHD -Path \"{}\" -PassThru -ErrorAction Stop",
            vhd_path.display()
        ));
        ps.add("$diskNumber = $disk.Number");

        for (i, part) in layout.partitions.iter().enumerate() {
            if !part.is_mountable() {
                continue;
            }

            let part_index = i + 1;

            let source_path_buf = layout.base_dir.join(&part.mountpoint);
            let source_path = source_path_buf.display();

            ps.add(format!(
                "$p = Get-Partition -DiskNumber $diskNumber -PartitionNumber {part_index}"
            ));

            let format_cmd = part.fs.build_format_command(&part.name, "$p")?.join(" ");

            ps.add(format_cmd);

            ps.add("Start-Sleep -Milliseconds 500");

            ps.add(format!("$vol = Get-Partition -DiskNumber $diskNumber -PartitionNumber {part_index} | Get-Volume"));

            ps.add("$targetPath = $vol.Path");

            ps.add(format!(
                "Copy-Item -Recurse -Force \"{source_path}\\*\" $targetPath\\"
            ));
        }

        ps.add("} finally {");
        ps.add(format!("Dismount-VHD -Path \"{}\"", vhd_path.display()));
        ps.add("}");

        Ok(ps)
    }

    pub fn run(&mut self, temp_dir: &Path) -> anyhow::Result<()> {
        let script_path = temp_dir.join("rimscript.ps1");
        let mut script = File::create(&script_path)?;
        let content = self.lines.join("\n");
        script.write_all(content.as_bytes())?;
        script.sync_all()?;
        drop(script);

        if self.debug {
            println!("==[Powershell Script]==\n{content}\n=====================");
        }

        let status = Command::new("powershell")
            .args(["-ExecutionPolicy", "Bypass", "-File"])
            .arg(script_path.canonicalize()?)
            .status()?;

        if !status.success() {
            anyhow::bail!("Powershell failed with exit code: {:?}", status.code());
        }

        Ok(())
    }
}
