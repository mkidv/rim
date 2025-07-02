use rimgen_layout::Filesystem;

pub trait FormatCommandBuilder {
    fn build_format_command(&self, label: &str, device: &str) -> anyhow::Result<Vec<String>>;
    fn required_binaries(&self) -> &'static [&'static str];
    fn ensure_binary_exists(cmd: &str) -> anyhow::Result<()> {
        if which::which(cmd).is_err() {
            anyhow::bail!(
                "Missing dependency: `{}` is not installed or not in PATH.",
                cmd
            );
        }
        Ok(())
    }
    fn validate_binaries(&self) -> anyhow::Result<()>;
}

impl FormatCommandBuilder for Filesystem {
    fn required_binaries(&self) -> &'static [&'static str] {
        match (std::env::consts::OS, self) {
            ("linux", Filesystem::Fat32) => &["mkfs.vfat"],
            ("linux", Filesystem::ExFat) => &["mkfs.exfat"],
            ("linux", Filesystem::Ntfs) => &["mkfs.ntfs"],
            ("linux", Filesystem::Ext4) => &["mkfs.ext4"],
            ("linux", Filesystem::Btrfs) => &["mkfs.btrfs"],
            ("linux", Filesystem::Xfs) => &["mkfs.xfs"],

            ("macos", Filesystem::Fat32) => &["newfs_msdos", "diskutil"],
            ("macos", Filesystem::ExFat) => &["diskutil"],
            ("macos", Filesystem::Ntfs) => &["mkntfs"],
            ("macos", Filesystem::Ext4) => &["mkfs.ext4"],

            ("windows", Filesystem::Fat32)
            | ("windows", Filesystem::ExFat)
            | ("windows", Filesystem::Ntfs) => &["powershell"],

            (_, Filesystem::Raw | Filesystem::None) => &[],
            _ => &[],
        }
    }

    fn validate_binaries(&self) -> anyhow::Result<()> {
        let missing: Vec<_> = self
            .required_binaries()
            .iter()
            .copied()
            .filter(|b| which::which(b).is_err())
            .collect();

        if !missing.is_empty() {
            anyhow::bail!(
                "Missing required tool(s) for formatting with '{}': {}",
                self,
                missing.join(", ")
            );
        }

        Ok(())
    }

    fn ensure_binary_exists(cmd: &str) -> anyhow::Result<()> {
        if which::which(cmd).is_err() {
            anyhow::bail!(
                "Missing dependency: `{}` is not installed or not in PATH.",
                cmd
            );
        }
        Ok(())
    }
    /// Builds the final command line to format the device with the given label.
    fn build_format_command(&self, label: &str, device: &str) -> anyhow::Result<Vec<String>> {
        let cmd: Vec<String> = match (std::env::consts::OS, self) {
            ("linux", Filesystem::Fat32) => {
                args!["mkfs.vfat", "-F", "32", "-n", label, device]
            }
            ("linux", Filesystem::ExFat) => args!["mkfs.exfat", "-n", label, device],

            ("linux", Filesystem::Ntfs) => args!["mkfs.ntfs", "-f", "-L", label, device],

            ("linux", Filesystem::Ext4) => args!["mkfs.ext4", "-F", "-L", label, device],

            ("linux", Filesystem::Btrfs) => args!["mkfs.btrfs", "-f", "-L", label, device],

            ("linux", Filesystem::Xfs) => args!["mkfs.xfs", "-f", "-L", label, device],

            ("macos", Filesystem::Fat32) => {
                if Self::ensure_binary_exists("newfs_msdos").is_ok() {
                    args!["newfs_msdos", "-F", "32", "-v", label, device]
                } else {
                    args!["diskutil", "eraseVolume", "MS-DOS", label, device]
                }
            }
            ("macos", Filesystem::Ntfs) => {
                if Self::ensure_binary_exists("mkntfs").is_ok() {
                    args!["mkntfs", "-f", "-L", label, device]
                } else {
                    anyhow::bail!("ntfs-3g not found on macOS.")
                }
            }
            ("macos", Filesystem::ExFat) => {
                args!["diskutil", "eraseVolume", "ExFAT", label, device]
            }
            ("macos", Filesystem::Ext4) => {
                if Self::ensure_binary_exists("mkfs.ext4").is_ok() {
                    args!["mkfs.ext4", "-F", "-L", label, device]
                } else {
                    anyhow::bail!("mkfs.ext4 not found. Try `brew install e2fsprogs`.")
                }
            }
            ("windows", Filesystem::Fat32 | Filesystem::ExFat | Filesystem::Ntfs) => {
                // Windows PowerShell only — not executed directly, used by rimgen's script generator
                let fs_upper = self.to_string().to_uppercase();
                args![
                    "Format-Volume",
                    "-Partition",
                    device,
                    "-FileSystem",
                    &fs_upper,
                    "-NewFileSystemLabel",
                    label,
                    "-Confirm:$false",
                    "-Force"
                ]
            }

            (_, Filesystem::Raw | Filesystem::None) => {
                anyhow::bail!("Cannot format RAW or NONE partitions.")
            }

            _ => anyhow::bail!("Unsupported fs {:?} on this platform", self),
        };

        Ok(cmd)
    }
}
