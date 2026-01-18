// SPDX-License-Identifier: MIT

use crate::layout::Layout;
use std::path::Path;
use std::process::Command;
use std::{fs::File, io::Write};

use crate::host::cmd_builder::FormatCommandBuilder;

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

    #[allow(dead_code)]
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

        // Cleanup
        sh.add(r#"trap "kpartx -d $LOOPDEV || true; losetup -d $LOOPDEV || true" EXIT"#);

        sh.add(r#"if [ "${EUID:-$(id -u)}" -ne 0 ]; then echo "[rimgen] please run as root"; exit 1; fi"#);
        sh.add(format!("LOOPDEV=$(losetup --find --show \"{image}\")"));
        sh.add("echo \"Using loop device: $LOOPDEV\"");
        sh.add("kpartx -a $LOOPDEV");
        sh.add("udevadm settle || true");

        for (i, part) in layout.partitions.iter().enumerate() {
            if !part.is_mountable() {
                continue;
            }

            let part_index = i + 1;
            sh.add("BASELOOP=$(basename $LOOPDEV)");
            let mapper_path = format!("/dev/mapper/${{BASELOOP}}p{part_index}");
            let mount_path = format!("/mnt/rimgen_{part_index}");
            let mountpoint = &part.mountpoint.as_deref().unwrap_or("");
            let source_path_buf = layout.base_dir.join(mountpoint);
            let source_path = source_path_buf.display();

            // Format
            let format_cmd = part
                .fs
                .build_format_command(&part.name, &mapper_path)?
                .join(" ");
            sh.add(format_cmd);

            if !mountpoint.is_empty() {
                // Mount, copy, unmount
                sh.add(format!("mkdir -p {mount_path}"));
                sh.add(format!("mount {mapper_path} {mount_path}"));
                sh.add(format!("if [ -d \"{source_path}\" ]; then rsync -a \"{source_path}/\" \"{mount_path}/\"; fi"));
                sh.add(format!("umount {mount_path}"));
            }
        }

        Ok(sh)
    }

    pub fn run(&mut self, temp_dir: &Path) -> anyhow::Result<()> {
        let script_path = temp_dir.join("rimscript.sh");
        let mut script = File::create(&script_path)?;
        let content = self.lines.join("\n");
        script.write_all(content.as_bytes())?;
        script.sync_all()?;
        drop(script);

        if self.debug {
            println!(
                "==[Linux Script]==\nset -x\n{}\n==================",
                content
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

    pub fn dry_mode(&self) -> anyhow::Result<()> {
        let content = self.lines.join("\n");

        println!(
            "==[Linux Script]==\nset -x\n{}\n==================",
            content
        );

        Ok(())
    }
}
