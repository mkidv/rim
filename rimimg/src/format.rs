// SPDX-License-Identifier: MIT

use rimio::prelude::*;
use std::fs::File;
use std::path::Path;

/// Supported disk image and container formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    /// Raw uncompressed disk image (.img, .raw)
    Raw,
    /// Microsoft Virtual Hard Disk fixed (.vhd)
    Vhd,
    /// VMware Virtual Machine Disk monolithicFlat (.vmdk)
    Vmdk,
    /// QEMU Copy-On-Write v2 linear/flat (.qcow2)
    Qcow2,
    /// VirtualBox Disk Image fixed 1.1 (.vdi)
    Vdi,
}

impl ImageFormat {
    /// Detect format from file path extension.
    pub fn from_path<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let ext = path
            .as_ref()
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        Self::from_extension(&ext)
    }

    /// Detect format from file extension string.
    pub fn from_extension(ext: &str) -> anyhow::Result<Self> {
        let clean = ext.trim_start_matches('.').to_ascii_lowercase();
        match clean.as_str() {
            "img" | "raw" => Ok(ImageFormat::Raw),
            "vhd" => Ok(ImageFormat::Vhd),
            "vmdk" => Ok(ImageFormat::Vmdk),
            "qcow2" => Ok(ImageFormat::Qcow2),
            "vdi" => Ok(ImageFormat::Vdi),
            "" => Ok(ImageFormat::Raw), // Default to RAW if no extension
            _ => anyhow::bail!("Unknown image format extension: .{}", ext),
        }
    }

    /// Default file extension for this format.
    pub fn default_extension(&self) -> &'static str {
        match self {
            ImageFormat::Raw => "img",
            ImageFormat::Vhd => "vhd",
            ImageFormat::Vmdk => "vmdk",
            ImageFormat::Qcow2 => "qcow2",
            ImageFormat::Vdi => "vdi",
        }
    }

    /// Detect format from a `std::fs::File` by checking size and magic bytes.
    pub fn from_file(file: &mut File) -> anyhow::Result<Self> {
        let mut io = StdRimIO::new(file);
        Self::from_io(&mut io)
    }

    /// Detect format by inspecting magic signatures from a `RimIO` stream.
    pub fn from_io(io: &mut dyn RimIO) -> anyhow::Result<Self> {
        let total_size = io.total_size().unwrap_or(0);

        // 1. Check QCOW2 magic at offset 0 (0x514649fb)
        if total_size >= 4 || total_size == 0 {
            let mut magic = [0u8; 4];
            if io.read_at(0, &mut magic).is_ok() && magic == [0x51, 0x46, 0x49, 0xfb] {
                return Ok(ImageFormat::Qcow2);
            }
        }

        // 2. Check VDI pre-header at offset 0 or signature at offset 64
        if total_size >= 68 || total_size == 0 {
            let mut pre = [0u8; 24];
            if io.read_at(0, &mut pre).is_ok() && pre.starts_with(b"<<< Oracle VM VirtualBox") {
                return Ok(ImageFormat::Vdi);
            }
            let mut sig = [0u8; 4];
            if io.read_at(64, &mut sig).is_ok() && sig == [0x7f, 0x10, 0xda, 0xbe] {
                return Ok(ImageFormat::Vdi);
            }
        }

        // 3. Check VMDK descriptor at offset 0
        if total_size >= 22 || total_size == 0 {
            let mut header = [0u8; 22];
            if io.read_at(0, &mut header).is_ok() && header.starts_with(b"# Disk DescriptorFile") {
                return Ok(ImageFormat::Vmdk);
            }
        }

        // 4. Check VHD cookie "conectix" in footer (last 512 bytes)
        if total_size >= 512 {
            let footer_offset = total_size - 512;
            let mut cookie = [0u8; 8];
            if io.read_at(footer_offset, &mut cookie).is_ok() && &cookie == b"conectix" {
                return Ok(ImageFormat::Vhd);
            }
        }

        // Fallback: treated as RAW
        Ok(ImageFormat::Raw)
    }
}

impl core::fmt::Display for ImageFormat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ImageFormat::Raw => write!(f, "RAW"),
            ImageFormat::Vhd => write!(f, "VHD"),
            ImageFormat::Vmdk => write!(f, "VMDK"),
            ImageFormat::Qcow2 => write!(f, "QCOW2"),
            ImageFormat::Vdi => write!(f, "VDI"),
        }
    }
}
