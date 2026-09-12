use crate::{ZipChecker, ZipFormatter, ZipInjector, ZipMeta, ZipResolver};
use alloc::vec;
use rimfs_core::{
    checker::FsChecker,
    formatter::FsFormatter,
    injector::FsTreeInjector,
    resolver::{FsNode, FsTreeResolver},
    testing::FailingRimIO,
};
use rimio::{MemRimIO, SliceRimIO};

#[test]
fn reformat_and_shorter_replacement_remove_stale_directory() {
    for full in [None, Some(false), Some(true)] {
        let mut disk = vec![0xA5; 65536 + 32];
        {
            // A bounded view must preserve bytes belonging to adjacent partitions.
            let mut io = MemRimIO::new(&mut disk[16..65552]);
            let meta = ZipMeta::default();
            ZipInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree(&mut FsNode::new_file("old.bin", vec![0x42; 16384]))
                .unwrap();
            let reopened = ZipMeta::from_io(&mut io).unwrap();
            assert_eq!(reopened.total_size, 65536);
            assert_eq!(
                ZipResolver::try_new(&mut io, &reopened)
                    .unwrap()
                    .read_file("old.bin")
                    .unwrap(),
                vec![0x42; 16384]
            );
            if let Some(full) = full {
                ZipFormatter::new(&mut io, &reopened).format(full).unwrap();
                assert!(
                    ZipResolver::try_new(&mut io, &reopened)
                        .unwrap()
                        .read_dir("/")
                        .unwrap()
                        .is_empty()
                );
                assert!(
                    ZipChecker::new(&mut io, &reopened)
                        .check_all()
                        .unwrap()
                        .findings
                        .is_empty()
                );
            }
            ZipInjector::new(&mut io, &reopened)
                .unwrap()
                .inject_tree(&mut FsNode::new_file("new.bin", b"new".to_vec()))
                .unwrap();
            let meta = ZipMeta::from_io(&mut io).unwrap();
            let mut resolver = ZipResolver::try_new(&mut io, &meta).unwrap();
            assert_eq!(resolver.read_dir("/").unwrap(), ["new.bin"]);
            assert_eq!(resolver.read_file("new.bin").unwrap(), b"new");
            assert!(!resolver.exists("old.bin"));
            drop(resolver);
            assert!(
                ZipChecker::new(&mut io, &meta)
                    .check_all()
                    .unwrap()
                    .findings
                    .is_empty()
            );
        }
        assert_eq!(&disk[..16], &[0xA5; 16]);
        assert_eq!(&disk[65552..], &[0xA5; 16]);
    }
}

#[test]
fn cleanup_errors_are_returned_and_from_io_rejects_invalid_input() {
    let meta = ZipMeta::default();
    let mut disk = vec![0xAA; 4096];
    let mut io = FailingRimIO::new(MemRimIO::new(&mut disk)).fail_write_at(22);
    assert!(ZipFormatter::new(&mut io, &meta).format(false).is_err());
    assert!(io.triggered);
    // Empty injection publishes an EOCD at zero, with the same cleanup boundary.
    assert!(ZipInjector::new(&mut io, &meta).unwrap().flush().is_err());
    assert!(ZipMeta::from_io(&mut SliceRimIO::new(&[0; 64])).is_err());
    assert!(ZipMeta::from_io(&mut SliceRimIO::new(&[0; 21])).is_err());
}

#[test]
fn already_null_storage_avoids_trailing_cleanup_writes() {
    let meta = ZipMeta::default();
    let mut disk = vec![0u8; 4096];
    // If cleanup wrote to offset 22 (the trailing area), this would fail.
    let mut io = FailingRimIO::new(MemRimIO::new(&mut disk)).fail_write_at(22);
    assert!(ZipFormatter::new(&mut io, &meta).format(false).is_ok());
    assert!(!io.triggered);

    // Same for ZipInjector
    let mut disk2 = vec![0u8; 4096];
    let mut io2 = FailingRimIO::new(MemRimIO::new(&mut disk2)).fail_write_at(22);
    assert!(ZipInjector::new(&mut io2, &meta).unwrap().flush().is_ok());
    assert!(!io2.triggered);
}

use core::sync::atomic::{AtomicUsize, Ordering};

struct AccessTrackingIO<'a, IO> {
    inner: IO,
    read_tail_count: &'a AtomicUsize,
    write_beyond_archive: &'a AtomicUsize,
    archive_boundary: &'a AtomicUsize,
}

impl<'a, IO: rimio::RimRead> rimio::RimRead for AccessTrackingIO<'a, IO> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> rimio::RimIOResult {
        if offset >= 22 {
            self.read_tail_count.fetch_add(1, Ordering::Relaxed);
        }
        self.inner.read_at(offset, buf)
    }

    fn total_size(&mut self) -> rimio::RimIOResult<u64> {
        self.inner.total_size()
    }
}

impl<'a, IO: rimio::RimWrite> rimio::RimWrite for AccessTrackingIO<'a, IO> {
    fn write_at(&mut self, offset: u64, data: &[u8]) -> rimio::RimIOResult {
        let boundary = self.archive_boundary.load(Ordering::Relaxed) as u64;
        if offset >= boundary {
            self.write_beyond_archive.fetch_add(1, Ordering::Relaxed);
        }
        self.inner.write_at(offset, data)
    }

    fn flush(&mut self) -> rimio::RimIOResult {
        self.inner.flush()
    }
}

impl<'a, IO: rimio::RimIO> rimio::RimIO for AccessTrackingIO<'a, IO> {
    fn set_offset(&mut self, offset: u64) -> u64 {
        self.inner.set_offset(offset)
    }

    fn partition_offset(&self) -> u64 {
        self.inner.partition_offset()
    }
}

#[test]
fn successive_flushes_avoid_redundant_tail_cleaning() {
    let meta = ZipMeta::default();
    let mut disk = vec![0xAA; 4096];
    let read_tail = AtomicUsize::new(0);
    let write_tail = AtomicUsize::new(0);
    let boundary = AtomicUsize::new(22);
    let mut io = AccessTrackingIO {
        inner: MemRimIO::new(&mut disk),
        read_tail_count: &read_tail,
        write_beyond_archive: &write_tail,
        archive_boundary: &boundary,
    };
    let mut injector = ZipInjector::new(&mut io, &meta).unwrap();

    // First flush must clean the dirty tail (offset 22..4096).
    injector.flush().unwrap();
    assert!(read_tail.load(Ordering::Relaxed) > 0);
    assert!(write_tail.load(Ordering::Relaxed) > 0);

    let reads_after_first = read_tail.load(Ordering::Relaxed);
    let writes_after_first = write_tail.load(Ordering::Relaxed);

    // Second flush on same injector must not perform any trailing cleaning.
    injector.flush().unwrap();
    assert_eq!(read_tail.load(Ordering::Relaxed), reads_after_first);
    assert_eq!(write_tail.load(Ordering::Relaxed), writes_after_first);

    // Appending a file and flushing again also avoids re-cleaning already-cleaned trailing storage.
    let mut src = SliceRimIO::new(b"hello");
    injector
        .write_file(
            "hello.txt",
            &mut src,
            5,
            &rimfs_core::resolver::attr::FileAttributes::new_file(),
        )
        .unwrap();

    // Set boundary to where the new archive content ends (~250 bytes).
    boundary.store(250, Ordering::Relaxed);
    let reads_before_third = read_tail.load(Ordering::Relaxed);
    let writes_before_third = write_tail.load(Ordering::Relaxed);
    injector.flush().unwrap();
    // Flush writes the new entry, CD, and EOCD, but must not re-read or re-write the trailing storage.
    assert_eq!(read_tail.load(Ordering::Relaxed), reads_before_third);
    assert_eq!(write_tail.load(Ordering::Relaxed), writes_before_third);

    drop(injector);
    let mut resolver = ZipResolver::try_new(&mut io, &meta).unwrap();
    assert_eq!(resolver.read_file("hello.txt").unwrap(), b"hello");
}
