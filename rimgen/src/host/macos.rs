// SPDX-License-Identifier: MIT

use crate::layout::Layout;
use std::path::Path;
use std::process::Command;
use std::{fs::File, io::Write};

use crate::host::cmd_builder::FormatCommandBuilder;

pub struct MacScript {
    lines: Vec<String>,
    debug: bool,
}

impl MacScript {
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
        let mut sh = MacScript::new();
        let image = img_path.display();

        sh.add(format!("DISK=$(hdiutil attach -imagekey diskimage-class=CRawDiskImage -nomount \"{image}\" | awk '{{print $1}}' | head -n 1)"));
        sh.add(r#"trap "hdiutil detach $DISK || true" EXIT"#);
        sh.add("echo \"Attached disk: $DISK\"");
        sh.add("diskutil list $DISK");

        for (i, part) in layout.partitions.iter().enumerate() {
            if !part.is_mountable() {
                continue;
            }

            let part_index = i + 1;
            let dev_part = format!("${{DISK}}s{part_index}");
            let mountpoint = &part.mountpoint.as_deref().unwrap_or("");
            let source_path_buf = layout.base_dir.join(mountpoint);
            let source_path = source_path_buf.display();

            // Format
            let format_cmd = part
                .fs
                .build_format_command(&part.name, &dev_part)?
                .join(" ");
            sh.add(format_cmd);

            if (!mountpoint.is_empty()) {
                // Mount
                sh.add(format!(
                "MOUNT=$(diskutil mount {dev_part} | grep 'mounted at' | sed 's/.*mounted at //')"
            ));
                sh.add(format!(
                    "if [ -d \"{source_path}\" ]; then rsync -a \"{source_path}/\" \"$MOUNT/\"; fi"
                ));
                sh.add("diskutil unmount \"$MOUNT\"");
            }
        }

        // Detach
        sh.add("hdiutil detach $DISK || true");

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
                "==[macOS Script]==\nset -x\n{}\n==================",
                content
            );
        }

        let status = Command::new("bash")
            .arg(script_path.canonicalize()?)
            .status()?;

        if !status.success() {
            anyhow::bail!("macOS script failed with exit code: {:?}", status.code());
        }

        Ok(())
    }

    pub fn dry_mode(&self) -> anyhow::Result<()> {
        let content = self.lines.join("\n");

        println!(
            "==[macOS Script]==\nset -x\n{}\n==================",
            content
        );

        Ok(())
    }
}
