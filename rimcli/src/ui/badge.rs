// SPDX-License-Identifier: MIT

//! Terminal badge and status tag formatting.

use colored::{ColoredString, Colorize};
use rimgen::Filesystem;

/// Return a styled badge for a given filesystem.
pub fn fs_badge(fs: &Filesystem) -> ColoredString {
    match fs {
        Filesystem::Fat32 => "FAT32".green().bold(),
        Filesystem::Fat16 => "FAT16".green().bold(),
        Filesystem::Fat12 => "FAT12".green(),
        Filesystem::Fat8 => "FAT8".green(),
        Filesystem::RimFat => "RimFAT".blue().bold(),
        Filesystem::ExFat => "exFAT".cyan().bold(),
        Filesystem::Ext4 => "EXT4".magenta().bold(),
        Filesystem::Ntfs => "NTFS".yellow().bold(),
        Filesystem::Btrfs => "Btrfs".blue(),
        Filesystem::Xfs => "XFS".blue(),
        Filesystem::Raw => "RAW".red().bold(),
        Filesystem::None => "None".normal(),
    }
}

/// Return a styled badge from a string identifier.
pub fn fs_name_badge(name: &str) -> ColoredString {
    match name.to_uppercase().as_str() {
        "FAT32" => "FAT32".green().bold(),
        "FAT16" => "FAT16".green().bold(),
        "FAT12" => "FAT12".green(),
        "RIMFAT" => "RimFAT".blue().bold(),
        "EXFAT" => "exFAT".cyan().bold(),
        "EXT4" | "EXT" => "EXT4".magenta().bold(),
        "NTFS" => "NTFS".yellow().bold(),
        "RAW" => "RAW".red().bold(),
        _ => name.cyan(),
    }
}
