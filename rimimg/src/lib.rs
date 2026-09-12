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
pub use io::{ImageIO, ImageReadIO, open_image_io, open_image_read_io};
pub use options::ImageOptions;

#[cfg(all(test, feature = "alloc"))]
mod tests {
    use super::*;
    use alloc::{vec, vec::Vec};
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

    #[test]
    fn test_image_read_io_opens_slice_without_mutable_container() {
        let format = ImageFormat::Qcow2;
        let raw_len = 1024 * 1024;
        let raw = patterned_raw(raw_len);
        let mut container = vec![0u8; container_capacity(format, raw_len)];

        {
            let mut container_io = MemRimIO::new(&mut container);
            let mut image = create_image_io(
                &mut container_io,
                raw_len as u64,
                format,
                ImageOptions::deterministic(0x51CE),
            )
            .unwrap();
            image.write_at(0, &raw).unwrap();
            image.finish().unwrap();
        }

        let mut slice = SliceRimIO::new(&container);
        assert_eq!(ImageFormat::from_read(&mut slice).unwrap(), format);

        let mut image = open_image_read_io(&mut slice).unwrap();
        assert_eq!(image.format(), format);
        assert_eq!(image.raw_len(), raw_len as u64);

        let mut actual = vec![0u8; raw_len];
        image.read_at(0, &mut actual).unwrap();
        assert_eq!(actual, raw);
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
            ImageFormat::Vdi => vdi::calculate_data_offset(raw_len as u64) as usize + raw_len,
        }
    }

    #[test]
    fn test_qcow2_sparse_unallocated_reads_zeroes() {
        let virtual_size = 10 * 1024 * 1024 * 1024u64;
        let mut storage = vec![0u8; 1024 * 1024];
        let mut mem_io = MemRimIO::new(&mut storage);

        let mut img = create_image_io(
            &mut mem_io,
            virtual_size,
            ImageFormat::Qcow2,
            ImageOptions::deterministic(1),
        )
        .unwrap();
        assert_eq!(img.raw_len(), virtual_size);

        let mut buf = [0xAAu8; 4096];
        img.read_at(0, &mut buf).unwrap();
        assert_eq!(buf, [0u8; 4096]);

        img.read_at(512 * 1024 * 1024, &mut buf).unwrap();
        assert_eq!(buf, [0u8; 4096]);

        img.read_at(virtual_size - 4096, &mut buf).unwrap();
        assert_eq!(buf, [0u8; 4096]);
    }

    #[test]
    fn test_qcow2_lazy_l2_allocation() {
        let virtual_size = 10 * 1024 * 1024 * 1024u64;
        let mut storage = vec![0u8; 1024 * 1024];
        let mut mem_io = MemRimIO::new(&mut storage);

        {
            let mut img = create_image_io(
                &mut mem_io,
                virtual_size,
                ImageFormat::Qcow2,
                ImageOptions::deterministic(1),
            )
            .unwrap();

            img.write_at(0, b"Payload at cluster 0").unwrap();

            img.write_at(1024 * 1024 * 1024, b"Payload at cluster 1GB")
                .unwrap();

            img.finish().unwrap();
        }

        let mut img = open_image_io(&mut mem_io).unwrap();
        let mut buf0 = [0u8; 20];
        img.read_at(0, &mut buf0).unwrap();
        assert_eq!(&buf0, b"Payload at cluster 0");

        let mut buf1g = [0u8; 22];
        img.read_at(1024 * 1024 * 1024, &mut buf1g).unwrap();
        assert_eq!(&buf1g, b"Payload at cluster 1GB");

        let mut buf512m = [0xAAu8; 4096];
        img.read_at(512 * 1024 * 1024, &mut buf512m).unwrap();
        assert_eq!(buf512m, [0u8; 4096]);
    }

    #[test]
    fn test_qcow2_multi_cluster_l1_table() {
        // 8 TiB virtual disk
        let virtual_size = 8 * 1024 * 1024 * 1024 * 1024u64;
        let (l1_size, n_rt, aligned) = qcow2::calculate_sparse_geometry(virtual_size);
        assert_eq!(aligned, virtual_size);
        // Each L2 table covers 512 MiB. 8 TiB has 16384 L2 tables.
        assert_eq!(l1_size, 16384);
        // L1 table size in bytes = 16384 * 8 = 131072 = 2 clusters of 64KB
        let n_l1_clusters = ((l1_size as u64) * 8).div_ceil(qcow2::CLUSTER_SIZE);
        assert_eq!(n_l1_clusters, 2);
        assert_eq!(n_rt, 1);

        // Test with sparse storage in memory
        let mut storage = vec![0u8; 512 * 1024];
        let mut mem_io = MemRimIO::new(&mut storage);

        {
            let mut img = create_image_io(
                &mut mem_io,
                virtual_size,
                ImageFormat::Qcow2,
                ImageOptions::deterministic(1),
            )
            .unwrap();
            assert_eq!(img.raw_len(), virtual_size);

            let write_offset = 5 * 1024 * 1024 * 1024 * 1024u64;
            img.write_at(write_offset, b"Data at 5 TiB").unwrap();
            img.finish().unwrap();
        }

        let mut img = open_image_io(&mut mem_io).unwrap();
        let mut buf = [0u8; 13];
        img.read_at(5 * 1024 * 1024 * 1024 * 1024u64, &mut buf)
            .unwrap();
        assert_eq!(&buf, b"Data at 5 TiB");
    }

    #[test]
    fn test_qcow2_shared_l1_rejection() {
        let virtual_size = 1024 * 1024u64;
        let mut storage = vec![0u8; 512 * 1024];
        let mut mem_io = MemRimIO::new(&mut storage);

        {
            let mut img = create_image_io(
                &mut mem_io,
                virtual_size,
                ImageFormat::Qcow2,
                ImageOptions::deterministic(1),
            )
            .unwrap();
            img.write_at(0, b"test").unwrap();
            img.finish().unwrap();
        }

        // Corrupt L1 entry by clearing bit 63 (OFLAG_COPIED) to simulate snapshot
        let header: qcow2::Qcow2Header = mem_io.read_struct(0).unwrap();
        let l1_off = header.l1_table_offset.get();
        let mut l1_entry_bytes = [0u8; 8];
        mem_io.read_at(l1_off, &mut l1_entry_bytes).unwrap();
        let mut l1_entry = u64::from_be_bytes(l1_entry_bytes);
        l1_entry &= !qcow2::QCOW_OFLAG_COPIED;
        mem_io.write_at(l1_off, &l1_entry.to_be_bytes()).unwrap();

        // Writing to shared L1 table must be rejected
        let mut img = open_image_io(&mut mem_io).unwrap();
        assert!(img.write_at(0, b"overwrite").is_err());
    }

    #[test]
    fn test_qcow2_shared_l2_rejection() {
        let virtual_size = 1024 * 1024u64;
        let mut storage = vec![0u8; 512 * 1024];
        let mut mem_io = MemRimIO::new(&mut storage);

        {
            let mut img = create_image_io(
                &mut mem_io,
                virtual_size,
                ImageFormat::Qcow2,
                ImageOptions::deterministic(1),
            )
            .unwrap();
            img.write_at(0, b"test").unwrap();
            img.finish().unwrap();
        }

        // Find L2 entry and clear bit 63 (OFLAG_COPIED)
        let header: qcow2::Qcow2Header = mem_io.read_struct(0).unwrap();
        let l1_off = header.l1_table_offset.get();
        let mut l1_bytes = [0u8; 8];
        mem_io.read_at(l1_off, &mut l1_bytes).unwrap();
        let l1_entry = u64::from_be_bytes(l1_bytes);
        let l2_off = l1_entry & qcow2::L2_OFFSET_MASK;

        let mut l2_bytes = [0u8; 8];
        mem_io.read_at(l2_off, &mut l2_bytes).unwrap();
        let mut l2_entry = u64::from_be_bytes(l2_bytes);
        l2_entry &= !qcow2::QCOW_OFLAG_COPIED;
        mem_io.write_at(l2_off, &l2_entry.to_be_bytes()).unwrap();

        // Writing to shared cluster must be rejected
        let mut img = open_image_io(&mut mem_io).unwrap();
        assert!(img.write_at(0, b"overwrite").is_err());
    }

    #[test]
    fn test_qcow2_zero_cluster_read_and_write() {
        let virtual_size = 1024 * 1024u64;
        let mut storage = vec![0u8; 512 * 1024];
        let mut mem_io = MemRimIO::new(&mut storage);

        {
            let mut img = create_image_io(
                &mut mem_io,
                virtual_size,
                ImageFormat::Qcow2,
                ImageOptions::deterministic(1),
            )
            .unwrap();
            img.write_at(0, b"Initial data").unwrap();
            img.finish().unwrap();
        }

        // Manually mark cluster 0 as ZERO cluster (set bit 0 QCOW_OFLAG_ZERO)
        let header: qcow2::Qcow2Header = mem_io.read_struct(0).unwrap();
        let l1_off = header.l1_table_offset.get();
        let mut l1_bytes = [0u8; 8];
        mem_io.read_at(l1_off, &mut l1_bytes).unwrap();
        let l1_entry = u64::from_be_bytes(l1_bytes);
        let l2_off = l1_entry & qcow2::L2_OFFSET_MASK;

        let mut l2_bytes = [0u8; 8];
        mem_io.read_at(l2_off, &mut l2_bytes).unwrap();
        let mut l2_entry = u64::from_be_bytes(l2_bytes);
        l2_entry |= qcow2::QCOW_OFLAG_ZERO;
        mem_io.write_at(l2_off, &l2_entry.to_be_bytes()).unwrap();

        // Reading zero cluster must return all zeroes
        {
            let mut img = open_image_io(&mut mem_io).unwrap();
            let mut buf = [0xAAu8; 12];
            img.read_at(0, &mut buf).unwrap();
            assert_eq!(buf, [0u8; 12]);

            // Writing to zero cluster should clear bit 0 and write payload
            img.write_at(0, b"Overwritten!").unwrap();
            img.finish().unwrap();
        }

        mem_io.read_at(l2_off, &mut l2_bytes).unwrap();
        let updated_l2 = u64::from_be_bytes(l2_bytes);
        assert_eq!(updated_l2 & qcow2::QCOW_OFLAG_ZERO, 0);

        let mut img = open_image_io(&mut mem_io).unwrap();
        let mut buf = [0u8; 12];
        img.read_at(0, &mut buf).unwrap();
        assert_eq!(&buf, b"Overwritten!");
    }

    #[test]
    fn test_qcow2_v3_header_validation() {
        let virtual_size = 1024 * 1024u64;
        let mut storage = vec![0u8; 512 * 1024];
        let mut mem_io = MemRimIO::new(&mut storage);

        qcow2::init_sparse_qcow2_layout(&mut mem_io, virtual_size).unwrap();

        // Valid v3 image opens fine
        assert!(open_image_io(&mut mem_io).is_ok());

        // Test incompatible features != 0 rejection
        let v3_offset = core::mem::size_of::<qcow2::Qcow2Header>() as u64;
        let mut v3_ext: qcow2::Qcow2HeaderV3Extension = mem_io.read_struct(v3_offset).unwrap();
        v3_ext.incompatible_features = zerocopy::byteorder::U64::new(1); // bit 0 (dirty)
        mem_io.write_struct(v3_offset, &v3_ext).unwrap();
        match open_image_io(&mut mem_io) {
            Err(e) => assert_eq!(e, RimImgError::UnsupportedFormat),
            Ok(_) => panic!("Expected error for incompatible_features != 0"),
        }

        v3_ext.incompatible_features = zerocopy::byteorder::U64::new(0);
        v3_ext.refcount_order = zerocopy::byteorder::U32::new(3); // 8-bit refcounts
        mem_io.write_struct(v3_offset, &v3_ext).unwrap();
        match open_image_io(&mut mem_io) {
            Err(e) => assert_eq!(e, RimImgError::UnsupportedFormat),
            Ok(_) => panic!("Expected error for refcount_order != 4"),
        }

        v3_ext.refcount_order = zerocopy::byteorder::U32::new(4);
        v3_ext.header_length = zerocopy::byteorder::U32::new(72);
        mem_io.write_struct(v3_offset, &v3_ext).unwrap();
        match open_image_io(&mut mem_io) {
            Err(RimImgError::InvalidHeader(_)) => {}
            other => panic!("Expected InvalidHeader error, got {:?}", other.is_err()),
        }
    }

    #[test]
    fn test_qcow2_virtual_eof_bounds() {
        // Reproduces finding H21: virtual size 513 with partial-cluster access crossing EOF
        let virtual_size = 513u64;
        let mut storage = vec![0u8; 512 * 1024];
        let mut mem_io = MemRimIO::new(&mut storage);

        let mut qcow = qcow2::create_sparse_qcow2_io(&mut mem_io, virtual_size).unwrap();

        // Writing 32 bytes at offset 512 would reach byte 544, exceeding virtual_size 513 -> OutOfBounds
        let data = [0xAAu8; 32];
        assert_eq!(qcow.write_at(512, &data), Err(RimIOError::OutOfBounds));

        // Reading 32 bytes at offset 512 -> OutOfBounds
        let mut buf = [0u8; 32];
        assert_eq!(qcow.read_at(512, &mut buf), Err(RimIOError::OutOfBounds));

        // Writing 1 byte at offset 512 -> exactly reaches byte 513 -> succeeds
        assert!(qcow.write_at(512, &[0x42]).is_ok());

        // Reading 1 byte at offset 512 -> succeeds and matches written byte
        let mut single_byte = [0u8; 1];
        assert!(qcow.read_at(512, &mut single_byte).is_ok());
        assert_eq!(single_byte[0], 0x42);

        // Accessing at offset 513 (EOF) -> OutOfBounds
        assert_eq!(
            qcow.read_at(513, &mut single_byte),
            Err(RimIOError::OutOfBounds)
        );
        assert_eq!(qcow.write_at(513, &[0x99]), Err(RimIOError::OutOfBounds));
    }

    #[test]
    fn test_vmdk_extent_offset_and_descriptor_validation() {
        // H19: VMDK extent offset must be 1 (DESCRIPTOR_SECTORS), not 0
        let disk_size = 2 * 1024 * 1024;
        let mut storage = vec![0u8; disk_size + 512];
        let mut io = MemRimIO::new(&mut storage);
        let mut raw_data = patterned_raw(disk_size);
        let mut src_io = MemRimIO::new(&mut raw_data);

        vmdk::wrap_raw_as_vmdk_io_with_progress(
            &mut src_io,
            &mut io,
            disk_size as u64,
            ImageOptions::deterministic(42),
            |_, _| {},
        )
        .unwrap();

        let mut desc_bytes = [0u8; 512];
        io.read_at(0, &mut desc_bytes).unwrap();
        let desc_str = core::str::from_utf8(&desc_bytes).unwrap();
        // Extent description must reference sector offset 1, not 0
        assert!(
            desc_str.contains("FLAT \"disk.vmdk\" 1"),
            "Descriptor should specify extent offset 1, got: {desc_str}"
        );

        // Tamper with descriptor magic -> unwrap must fail with InvalidHeader
        let mut corrupt_storage = storage.clone();
        corrupt_storage[0..4].copy_from_slice(b"XXXX");
        let mut corrupt_io = MemRimIO::new(&mut corrupt_storage);
        let mut dst = vec![0u8; disk_size];
        let mut dst_io = MemRimIO::new(&mut dst);
        assert_eq!(
            vmdk::unwrap_vmdk_io(&mut corrupt_io, &mut dst_io),
            Err(RimImgError::InvalidHeader("Invalid VMDK descriptor"))
        );
    }

    #[test]
    fn test_vhd_disk_type_validation() {
        // H19: VHD must reject non-fixed disk types (e.g. dynamic = 3)
        let disk_size = 512u64;
        let mut footer = vhd::VhdFooter::new_fixed(disk_size, ImageOptions::deterministic(1));
        assert!(footer.validate());

        // Change disk_type to 3 (dynamic) and recalculate checksum
        footer.disk_type = zerocopy::byteorder::U32::new(3);
        let sum = footer.compute_checksum();
        footer.checksum = zerocopy::byteorder::U32::new(sum);

        assert!(!footer.validate(), "Dynamic VHD must fail validate()");

        let mut storage = vec![0u8; (disk_size + vhd::VHD_FOOTER_SIZE) as usize];
        let mut io = MemRimIO::new(&mut storage);
        io.write_struct(disk_size, &footer).unwrap();

        let mut dst = vec![0u8; disk_size as usize];
        let mut dst_io = MemRimIO::new(&mut dst);
        assert_eq!(
            vhd::unwrap_vhd_io(&mut io, &mut dst_io),
            Err(RimImgError::UnsupportedFormat)
        );
    }

    #[test]
    fn test_vdi_dynamic_data_offset_and_overlap_rejection() {
        // H20: Small disk should use 1MB data offset
        let small_disk = 10 * 1024 * 1024u64;
        assert_eq!(vdi::calculate_data_offset(small_disk), 1024 * 1024);

        // Huge disk (e.g. 500GB -> 500,000 blocks * 4 = 2,000,000 bytes > 1MB)
        let huge_disk = 500 * 1024 * 1024 * 1024u64;
        let huge_offset = vdi::calculate_data_offset(huge_disk);
        assert!(huge_offset >= 512 + (500 * 1024) * 4);
        assert_eq!(huge_offset % (1024 * 1024), 0); // 1MB aligned

        // Overlapping header rejection
        let mut header = vdi::VdiHeader::new_fixed(small_disk, [0u8; 16]);
        // Corrupt offset_data to overlap block map
        header.offset_data = zerocopy::byteorder::U32::new(512);
        assert_eq!(
            vdi::validate_vdi_header(&header),
            Err(RimImgError::Corrupted("VDI data offset overlaps block map"))
        );
    }
}
