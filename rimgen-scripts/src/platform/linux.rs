// rimgen/platform/windows/lin_script.rs

use rimgen_layout::*;
use rimgen_output::*;
use std::path::Path;
use std::process::Command;
use std::{fs::File, io::Write};

use crate::cmd_builder::FormatCommandBuilder;

pub struct LinScript {
    lines: Vec<String>,
    debug: bool,
}

impl LinScript {
    fn new() -> Self {
        Self {
            lines: vec!["#!/bin/bash".into(), "set -euo pipefail".into()],
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

    pub fn new_from(layout: &Layout, img_path: &Path) -> anyhow::Result<Self> {
        let mut sh = LinScript::new();

        let image = img_path.display();

        sh.add(format!("LOOPDEV=$(losetup --find --show \"{image}\")"));
        sh.add("echo \"Using loop device: $LOOPDEV\"");
        sh.add("kpartx -a $LOOPDEV");
        sh.add("sleep 1");

        for (i, part) in layout.partitions.iter().enumerate() {
            if !part.is_mountable() {
                continue;
            }

            let part_index = i + 1;
            sh.add("BASELOOP=$(basename $LOOPDEV)");
            let mapper_path = format!("/dev/mapper/${{BASELOOP}}p{part_index}");
            let mount_path = format!("/mnt/rimgen_{part_index}");
            let source_path_buf = layout.base_dir.join(&part.mountpoint);
            let source_path = source_path_buf.display();

            // Format
            let format_cmd = part
                .fs
                .build_format_command(&part.name, &mapper_path)?
                .join(" ");
            sh.add(format_cmd);

            // Mount, copy, unmount
            sh.add(format!("mkdir -p {mount_path}"));
            sh.add(format!("mount {mapper_path} {mount_path}"));
            sh.add(format!("if [ -d \"{source_path}\" ]; then rsync -a \"{source_path}/\" \"{mount_path}/\"; fi"));
            sh.add(format!("umount {mount_path}"));
        }
        // Cleanup
        sh.add("kpartx -d $LOOPDEV");
        sh.add("losetup -d $LOOPDEV");

        Ok(sh)
    }

    pub fn run(&mut self, temp_dir: &Path) -> anyhow::Result<()> {
        let script_path = temp_dir.join("rimscript.sh");
        let mut script = File::create(&script_path)?;
        script.write_all(self.lines.join("\n").as_bytes())?;
        script.sync_all()?;
        drop(script);

        if self.debug {
            println!(
                "==[Linux Script]==\n{}\n==================",
                self.lines.join("\n")
            );
        }

        let status = Command::new("bash")
            .arg(script_path.canonicalize()?)
            .status()?;

        if !status.success() {
            anyhow::bail!("Shell script failed with exit code: {:?}", status.code());
        }

        Ok(())
    }
}
