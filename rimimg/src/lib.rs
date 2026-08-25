// SPDX-License-Identifier: MIT

//! `rimimg`: Disk image and container formats for the RIM ecosystem.
//!
//! Provides support for encapsulating raw disk images into various virtual machine
//! disk container formats (VHD fixed, VMDK monolithicFlat, QCOW2 v2 linear, VDI fixed),
//! detecting formats from extensions or magic bytes, and converting between formats.

pub mod convert;
pub mod format;
pub mod qcow2;
pub mod raw;
pub mod vdi;
pub mod vhd;
pub mod vmdk;

pub use convert::{
    convert, convert_explicit, convert_explicit_with_progress, convert_with_progress, unwrap,
    unwrap_with_progress, wrap, wrap_with_progress,
};
pub use format::ImageFormat;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    fn create_dummy_raw_image(path: &std::path::Path, size_bytes: usize) {
        let mut f = File::create(path).unwrap();
        let pattern: Vec<u8> = (0..size_bytes).map(|i| (i % 251) as u8).collect();
        f.write_all(&pattern).unwrap();
        f.sync_all().unwrap();
    }

    #[test]
    fn test_format_detection_from_path() {
        assert_eq!(
            ImageFormat::from_path("disk.img").unwrap(),
            ImageFormat::Raw
        );
        assert_eq!(
            ImageFormat::from_path("disk.raw").unwrap(),
            ImageFormat::Raw
        );
        assert_eq!(
            ImageFormat::from_path("disk.vhd").unwrap(),
            ImageFormat::Vhd
        );
        assert_eq!(
            ImageFormat::from_path("disk.vmdk").unwrap(),
            ImageFormat::Vmdk
        );
        assert_eq!(
            ImageFormat::from_path("disk.qcow2").unwrap(),
            ImageFormat::Qcow2
        );
        assert_eq!(
            ImageFormat::from_path("disk.vdi").unwrap(),
            ImageFormat::Vdi
        );
        assert!(ImageFormat::from_path("disk.xyz").is_err());
    }

    #[test]
    fn test_raw_vhd_roundtrip() {
        let dir = tempdir().unwrap();
        let raw_src = dir.path().join("source.img");
        let vhd_path = dir.path().join("target.vhd");
        let raw_dst = dir.path().join("restored.img");

        let size = 2 * 1024 * 1024; // 2MB
        create_dummy_raw_image(&raw_src, size);

        // Wrap to VHD
        wrap(&raw_src, &vhd_path, ImageFormat::Vhd).unwrap();
        assert!(vhd_path.exists());

        // Validate VHD footer
        let mut vhd_file = File::open(&vhd_path).unwrap();
        let mut io = rimio::prelude::StdRimIO::new(&mut vhd_file);
        assert_eq!(ImageFormat::from_io(&mut io).unwrap(), ImageFormat::Vhd);

        // Unwrap back to RAW
        unwrap(&vhd_path, &raw_dst, ImageFormat::Vhd).unwrap();
        assert!(raw_dst.exists());

        let src_bytes = std::fs::read(&raw_src).unwrap();
        let dst_bytes = std::fs::read(&raw_dst).unwrap();
        assert_eq!(src_bytes, dst_bytes);
    }

    #[test]
    fn test_raw_vmdk_roundtrip() {
        let dir = tempdir().unwrap();
        let raw_src = dir.path().join("source.img");
        let vmdk_path = dir.path().join("target.vmdk");
        let raw_dst = dir.path().join("restored.img");

        let size = 1024 * 1024; // 1MB
        create_dummy_raw_image(&raw_src, size);

        wrap(&raw_src, &vmdk_path, ImageFormat::Vmdk).unwrap();
        assert!(vmdk_path.exists());

        let mut vmdk_file = File::open(&vmdk_path).unwrap();
        let mut io = rimio::prelude::StdRimIO::new(&mut vmdk_file);
        assert_eq!(ImageFormat::from_io(&mut io).unwrap(), ImageFormat::Vmdk);

        unwrap(&vmdk_path, &raw_dst, ImageFormat::Vmdk).unwrap();
        assert_eq!(
            std::fs::read(&raw_src).unwrap(),
            std::fs::read(&raw_dst).unwrap()
        );
    }

    #[test]
    fn test_raw_qcow2_roundtrip() {
        let dir = tempdir().unwrap();
        let raw_src = dir.path().join("source.img");
        let qcow2_path = dir.path().join("target.qcow2");
        let raw_dst = dir.path().join("restored.img");

        let size = 1024 * 1024; // 1MB
        create_dummy_raw_image(&raw_src, size);

        wrap(&raw_src, &qcow2_path, ImageFormat::Qcow2).unwrap();
        assert!(qcow2_path.exists());

        let mut qcow2_file = File::open(&qcow2_path).unwrap();
        let mut io = rimio::prelude::StdRimIO::new(&mut qcow2_file);
        assert_eq!(ImageFormat::from_io(&mut io).unwrap(), ImageFormat::Qcow2);

        unwrap(&qcow2_path, &raw_dst, ImageFormat::Qcow2).unwrap();
        assert_eq!(
            std::fs::read(&raw_src).unwrap(),
            std::fs::read(&raw_dst).unwrap()
        );
    }

    #[test]
    fn test_raw_vdi_roundtrip() {
        let dir = tempdir().unwrap();
        let raw_src = dir.path().join("source.img");
        let vdi_path = dir.path().join("target.vdi");
        let raw_dst = dir.path().join("restored.img");

        let size = 2 * 1024 * 1024; // 2MB
        create_dummy_raw_image(&raw_src, size);

        wrap(&raw_src, &vdi_path, ImageFormat::Vdi).unwrap();
        assert!(vdi_path.exists());

        let mut vdi_file = File::open(&vdi_path).unwrap();
        let mut io = rimio::prelude::StdRimIO::new(&mut vdi_file);
        assert_eq!(ImageFormat::from_io(&mut io).unwrap(), ImageFormat::Vdi);

        unwrap(&vdi_path, &raw_dst, ImageFormat::Vdi).unwrap();
        assert_eq!(
            std::fs::read(&raw_src).unwrap(),
            std::fs::read(&raw_dst).unwrap()
        );
    }

    #[test]
    fn test_conversion_between_formats() {
        let dir = tempdir().unwrap();
        let raw_src = dir.path().join("source.img");
        let vhd_path = dir.path().join("disk.vhd");
        let qcow2_path = dir.path().join("disk.qcow2");
        let vdi_path = dir.path().join("disk.vdi");
        let raw_final = dir.path().join("final.img");

        let size = 2 * 1024 * 1024;
        create_dummy_raw_image(&raw_src, size);

        // RAW -> VHD
        convert(&raw_src, &vhd_path).unwrap();
        // VHD -> QCOW2
        convert(&vhd_path, &qcow2_path).unwrap();
        // QCOW2 -> VDI
        convert(&qcow2_path, &vdi_path).unwrap();
        // VDI -> RAW
        convert(&vdi_path, &raw_final).unwrap();

        assert_eq!(
            std::fs::read(&raw_src).unwrap(),
            std::fs::read(&raw_final).unwrap()
        );
    }
}
