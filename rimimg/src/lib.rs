// SPDX-License-Identifier: MIT

//! `rimimg`: Disk image and container formats for the RIM ecosystem.
//!
//! Provides support for encapsulating raw disk images into various virtual machine
//! disk container formats (VHD fixed, VMDK monolithicFlat, QCOW2 v2 linear, VDI fixed),
//! detecting formats from extensions or magic bytes, and converting between formats.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod convert;
pub mod errors;
pub mod format;
pub mod io;
pub mod options;
pub mod qcow2;
pub mod raw;
pub mod vdi;
pub mod vhd;
pub mod vmdk;

pub use convert::{unwrap_io, unwrap_io_with_progress};
#[cfg(feature = "alloc")]
pub use convert::{wrap_io, wrap_io_with_progress};
pub use errors::*;
pub use format::ImageFormat;
#[cfg(feature = "alloc")]
pub use io::create_image_io;
pub use io::{ImageIO, open_image_io};
pub use options::ImageOptions;

#[cfg(test)]
mod tests {
    use super::*;
    use rimio::prelude::*;

    fn patterned_raw(size_bytes: usize) -> Vec<u8> {
        (0..size_bytes).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn test_format_detection_from_extension() {
        assert_eq!(
            ImageFormat::from_extension("img").unwrap(),
            ImageFormat::Raw
        );
        assert_eq!(
            ImageFormat::from_extension(".raw").unwrap(),
            ImageFormat::Raw
        );
        assert_eq!(
            ImageFormat::from_extension("VHD").unwrap(),
            ImageFormat::Vhd
        );
        assert_eq!(
            ImageFormat::from_extension("vmdk").unwrap(),
            ImageFormat::Vmdk
        );
        assert_eq!(
            ImageFormat::from_extension("qcow2").unwrap(),
            ImageFormat::Qcow2
        );
        assert_eq!(
            ImageFormat::from_extension("vdi").unwrap(),
            ImageFormat::Vdi
        );
        assert!(ImageFormat::from_extension("xyz").is_err());
    }

    #[test]
    fn test_image_options_deterministic_uuid_bits() {
        let options = ImageOptions::deterministic(0xDEAD_BEEF);

        assert_eq!(options.unique_id[6] & 0xF0, 0x40);
        assert_eq!(options.unique_id[8] & 0xC0, 0x80);
        assert_eq!(options.timestamp_seconds, 0);
    }

    #[test]
    fn test_format_detection_from_memory_io() {
        let mut qcow2 = [0u8; 512];
        qcow2[..4].copy_from_slice(&[0x51, 0x46, 0x49, 0xfb]);
        let mut io = MemRimIO::new(&mut qcow2);
        assert_eq!(ImageFormat::from_io(&mut io).unwrap(), ImageFormat::Qcow2);

        let mut vdi = [0u8; 512];
        vdi[..24].copy_from_slice(b"<<< Oracle VM VirtualBox");
        let mut io = MemRimIO::new(&mut vdi);
        assert_eq!(ImageFormat::from_io(&mut io).unwrap(), ImageFormat::Vdi);

        let mut raw = [0u8; 512];
        let mut io = MemRimIO::new(&mut raw);
        assert_eq!(ImageFormat::from_io(&mut io).unwrap(), ImageFormat::Raw);
    }

    #[test]
    fn test_raw_memory_roundtrip() {
        let mut raw_src = patterned_raw(4096);
        let mut raw_dst = vec![0u8; raw_src.len()];
        let mut src_io = MemRimIO::new(&mut raw_src);
        let mut dst_io = MemRimIO::new(&mut raw_dst);

        raw::wrap_raw_as_raw_io(&mut src_io, &mut dst_io, 4096).unwrap();

        assert_eq!(raw_src, raw_dst);
    }

    #[test]
    fn test_vhd_memory_roundtrip() {
        let mut raw_src = patterned_raw(4096);
        let mut container = vec![0u8; raw_src.len() + vhd::VHD_FOOTER_SIZE as usize];
        let mut src_io = MemRimIO::new(&mut raw_src);
        let mut container_io = MemRimIO::new(&mut container);

        vhd::wrap_raw_as_vhd_io(
            &mut src_io,
            &mut container_io,
            4096,
            ImageOptions::deterministic(1),
        )
        .unwrap();

        assert_eq!(
            ImageFormat::from_io(&mut container_io).unwrap(),
            ImageFormat::Vhd
        );

        let mut raw_dst = vec![0u8; 4096];
        let mut dst_io = MemRimIO::new(&mut raw_dst);
        vhd::unwrap_vhd_io(&mut container_io, &mut dst_io).unwrap();

        assert_eq!(raw_src, raw_dst);
    }

    #[test]
    fn test_vmdk_memory_roundtrip() {
        let mut raw_src = patterned_raw(1024 * 1024);
        let mut container = vec![0u8; raw_src.len() + vmdk::SECTOR_SIZE as usize];
        let mut src_io = MemRimIO::new(&mut raw_src);
        let mut container_io = MemRimIO::new(&mut container);

        wrap_io(
            &mut src_io,
            &mut container_io,
            1024 * 1024,
            ImageFormat::Vmdk,
            ImageOptions::deterministic(2),
        )
        .unwrap();

        assert_eq!(
            ImageFormat::from_io(&mut container_io).unwrap(),
            ImageFormat::Vmdk
        );

        let mut raw_dst = vec![0u8; 1024 * 1024];
        let mut dst_io = MemRimIO::new(&mut raw_dst);
        vmdk::unwrap_vmdk_io(&mut container_io, &mut dst_io).unwrap();

        assert_eq!(raw_src, raw_dst);
    }

    #[test]
    fn test_qcow2_memory_roundtrip() {
        let raw_len = 1024 * 1024;
        let l1_size = 1usize;
        let data_start = (4 + l1_size) * qcow2::CLUSTER_SIZE as usize;
        let mut raw_src = patterned_raw(raw_len);
        let mut container = vec![0u8; data_start + raw_len];
        let mut src_io = MemRimIO::new(&mut raw_src);
        let mut container_io = MemRimIO::new(&mut container);

        qcow2::wrap_raw_as_qcow2_io(&mut src_io, &mut container_io, raw_len as u64).unwrap();

        assert_eq!(
            ImageFormat::from_io(&mut container_io).unwrap(),
            ImageFormat::Qcow2
        );

        let mut raw_dst = vec![0u8; raw_len];
        let mut dst_io = MemRimIO::new(&mut raw_dst);
        qcow2::unwrap_qcow2_io(&mut container_io, &mut dst_io).unwrap();

        assert_eq!(raw_src, raw_dst);
    }

    #[test]
    fn test_vdi_memory_roundtrip() {
        let raw_len = 2 * 1024 * 1024;
        let mut raw_src = patterned_raw(raw_len);
        let mut container = vec![0u8; vdi::DATA_OFFSET as usize + raw_len];
        let mut src_io = MemRimIO::new(&mut raw_src);
        let mut container_io = MemRimIO::new(&mut container);

        vdi::wrap_raw_as_vdi_io(
            &mut src_io,
            &mut container_io,
            raw_len as u64,
            ImageOptions::deterministic(3),
        )
        .unwrap();

        assert_eq!(
            ImageFormat::from_io(&mut container_io).unwrap(),
            ImageFormat::Vdi
        );

        let mut raw_dst = vec![0u8; raw_len];
        let mut dst_io = MemRimIO::new(&mut raw_dst);
        vdi::unwrap_vdi_io(&mut container_io, &mut dst_io).unwrap();

        assert_eq!(raw_src, raw_dst);
    }

    #[test]
    fn test_image_io_create_open_roundtrip() {
        let cases = [
            (ImageFormat::Raw, 1024 * 1024),
            (ImageFormat::Vhd, 1024 * 1024),
            (ImageFormat::Vmdk, 1024 * 1024),
            (ImageFormat::Qcow2, 1024 * 1024),
            (ImageFormat::Vdi, 2 * 1024 * 1024),
        ];

        for (format, raw_len) in cases {
            let raw = patterned_raw(raw_len);
            let mut container = vec![0u8; container_capacity(format, raw_len)];
            let mut container_io = MemRimIO::new(&mut container);

            {
                let mut image = create_image_io(
                    &mut container_io,
                    raw_len as u64,
                    format,
                    ImageOptions::deterministic(0xA11CE),
                )
                .unwrap();
                image.write_at(0, &raw).unwrap();
                image.finish().unwrap();
            }

            assert_eq!(ImageFormat::from_io(&mut container_io).unwrap(), format);

            let mut image = open_image_io(&mut container_io).unwrap();
            assert_eq!(image.format(), format);
            assert_eq!(image.raw_len(), raw_len as u64);

            let mut actual = vec![0u8; raw_len];
            image.read_at(0, &mut actual).unwrap();
            assert_eq!(actual, raw);
        }
    }

    fn container_capacity(format: ImageFormat, raw_len: usize) -> usize {
        match format {
            ImageFormat::Raw => raw_len,
            ImageFormat::Vhd => raw_len + vhd::VHD_FOOTER_SIZE as usize,
            ImageFormat::Vmdk => raw_len + vmdk::SECTOR_SIZE as usize,
            ImageFormat::Qcow2 => {
                let virtual_size =
                    (raw_len as u64).div_ceil(qcow2::CLUSTER_SIZE) * qcow2::CLUSTER_SIZE;
                let num_clusters = virtual_size / qcow2::CLUSTER_SIZE;
                let l2_entries = qcow2::CLUSTER_SIZE / 8;
                let l1_size = num_clusters.div_ceil(l2_entries) as usize;
                ((4 + l1_size) * qcow2::CLUSTER_SIZE as usize) + virtual_size as usize
            }
            ImageFormat::Vdi => vdi::DATA_OFFSET as usize + raw_len,
        }
    }
}
