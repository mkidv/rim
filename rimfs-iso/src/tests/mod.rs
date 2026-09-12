pub mod conformance;
pub mod spec_compliance;

use super::*;
#[cfg(feature = "std")]
use rimfs_core::StdResolver;
use rimfs_core::checker::{FsChecker, VerifyReport};
use rimfs_core::formatter::FsFormatter;
use rimfs_core::injector::FsTreeInjector;
use rimfs_core::resolver::FsTreeResolver;
use rimfs_core::resolver::attr::FileAttributes;
use rimfs_core::resolver::node::FsNode;
use rimfs_core::testing::{
    ExpectedFile, ExpectedLink, assert_exists, assert_files, assert_has_error, assert_no_findings,
    assert_symlinks, file, file_with_attr,
};
use rimio::{MemRimIO, RimRead, RimWrite, SliceRimIO};

#[test]
fn test_iso_pvd_corruption_detection() {
    let meta = IsoMeta::default();
    let mut disk_buf = alloc::vec![0u8; 100 * ISO_SECTOR_SIZE];
    let mut io = MemRimIO::new(&mut disk_buf);

    let mut formatter = IsoFormatter::new(&mut io, &meta);
    formatter.format(false).unwrap();

    io.write_at(16 * ISO_SECTOR_SIZE as u64 + 1, b"X").unwrap();

    let mut checker = IsoChecker::new(&mut io, &meta);
    let mut report = VerifyReport::default();
    checker
        .check_boot(&IsoCheckerOptions::default(), &mut report)
        .unwrap();
    assert_has_error(&report, "ISO.PVD");
}

#[test]
fn test_iso_files_dirs_and_joliet_rockridge() {
    let meta = IsoMeta::default();
    let mut disk_buf = alloc::vec![0u8; 200 * ISO_SECTOR_SIZE];
    let mut io = MemRimIO::new(&mut disk_buf);

    let mut custom_attr = FileAttributes::new_file();
    custom_attr.mode = Some(0o755);
    custom_attr.uid = Some(1001);
    custom_attr.gid = Some(1001);

    let mut tree = FsNode::new_container(alloc::vec![
        FsNode::new_dir("docs"),
        file_with_attr("hello.txt", b"Hello ISO 9660!", custom_attr),
        file("docs/unicode_étudiant.txt", b"Donnees Joliet"),
        FsNode::new_symlink("link_to_hello", "hello.txt"),
    ]);

    let mut injector = IsoInjector::new(&mut io, &meta).unwrap();
    injector.inject_tree(&mut tree).unwrap();

    let mut checker = IsoChecker::new(&mut io, &meta);
    let report = checker.check_all().unwrap();
    assert_no_findings(&report);

    let mut resolver = IsoResolver::new(&mut io, &meta);
    assert_exists(
        &mut resolver,
        &[
            "hello.txt",
            "docs",
            "docs/unicode_étudiant.txt",
            "link_to_hello",
        ],
    );
    assert_files(
        &mut resolver,
        &[
            ExpectedFile {
                path: "hello.txt",
                bytes: b"Hello ISO 9660!",
            },
            ExpectedFile {
                path: "docs/unicode_étudiant.txt",
                bytes: b"Donnees Joliet",
            },
        ],
    );
    assert_symlinks(
        &mut resolver,
        &[ExpectedLink {
            path: "link_to_hello",
            target: "hello.txt",
        }],
    );

    let attr = resolver.read_attributes("hello.txt").unwrap();
    assert_eq!(attr.mode, Some(0o100755));
    assert_eq!(attr.uid, Some(1001));
    assert_eq!(attr.gid, Some(1001));
}

#[cfg(feature = "std")]
#[test]
fn test_iso_streaming_inject_from_std_resolver() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir(root.join("docs")).unwrap();
    std::fs::write(root.join("hello.txt"), b"Hello from resolver").unwrap();
    std::fs::write(root.join("docs").join("notes.txt"), b"Nested resolver file").unwrap();

    let meta = IsoMeta::default();
    let mut disk_buf = alloc::vec![0u8; 200 * ISO_SECTOR_SIZE];
    let mut io = MemRimIO::new(&mut disk_buf);
    let mut resolver = StdResolver::new();
    let source = format!("{}/*", root.display());

    let mut injector = IsoInjector::new(&mut io, &meta).unwrap();
    let counts = injector
        .inject_tree_from_resolver(&mut resolver, &source)
        .unwrap();

    assert_eq!(counts.dirs, 1);
    assert_eq!(counts.files, 2);
    assert_eq!(counts.bytes, 39);

    let mut checker = IsoChecker::new(&mut io, &meta);
    let report = checker.check_all().unwrap();
    assert_no_findings(&report);

    let mut iso = IsoResolver::new(&mut io, &meta);
    assert_exists(&mut iso, &["hello.txt", "docs", "docs/notes.txt"]);
    assert_files(
        &mut iso,
        &[
            ExpectedFile {
                path: "hello.txt",
                bytes: b"Hello from resolver",
            },
            ExpectedFile {
                path: "docs/notes.txt",
                bytes: b"Nested resolver file",
            },
        ],
    );
}

#[test]
fn test_iso_el_torito_efi_boot() {
    let meta = IsoMeta {
        boot_efi: Some(b"MZ_DUMMY_BOOTX64_EFI_BINARY_PAYLOAD".to_vec()),
        ..Default::default()
    };

    let mut disk_buf = alloc::vec![0u8; 1500 * ISO_SECTOR_SIZE];
    let mut io = MemRimIO::new(&mut disk_buf);

    let mut tree = FsNode::new_container(alloc::vec![file("readme.txt", b"Bootable ISO Image",)]);

    let mut injector = IsoInjector::new(&mut io, &meta).unwrap();
    injector.inject_tree(&mut tree).unwrap();

    let mut checker = IsoChecker::new(&mut io, &meta);
    let report = checker.check_all().unwrap();
    assert_no_findings(&report);

    let mut resolver = IsoResolver::new(&mut io, &meta);
    assert_files(
        &mut resolver,
        &[ExpectedFile {
            path: "readme.txt",
            bytes: b"Bootable ISO Image",
        }],
    );

    let mut efi_image_reader = resolver
        .open_el_torito_efi_image()
        .unwrap()
        .expect("EFI image expected");
    assert_eq!(efi_image_reader.total_size().unwrap(), 1440 * 1024);
    let mut efi_lead_buf = [0u8; 32];
    efi_image_reader.read_at(0, &mut efi_lead_buf).unwrap();
    assert_eq!(efi_lead_buf[0], 0xEB);
}

use ::core::sync::atomic::{AtomicUsize, Ordering};
use alloc::sync::Arc;

struct CountingRimIO<T> {
    inner: T,
    bytes_counter: Arc<AtomicUsize>,
    reads_counter: Arc<AtomicUsize>,
}

impl<T: RimRead> CountingRimIO<T> {
    fn new(inner: T, bytes_counter: Arc<AtomicUsize>, reads_counter: Arc<AtomicUsize>) -> Self {
        Self {
            inner,
            bytes_counter,
            reads_counter,
        }
    }
}

impl<T: RimRead> RimRead for CountingRimIO<T> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> rimio::prelude::RimIOResult {
        self.bytes_counter.fetch_add(buf.len(), Ordering::SeqCst);
        self.reads_counter.fetch_add(1, Ordering::SeqCst);
        self.inner.read_at(offset, buf)
    }

    fn total_size(&mut self) -> rimio::prelude::RimIOResult<u64> {
        self.inner.total_size()
    }
}

#[test]
fn test_nested_lazy_iso_in_iso() {
    let child_meta = IsoMeta::default();
    let mut child_buf = alloc::vec![0u8; 100 * ISO_SECTOR_SIZE];
    let mut child_io = MemRimIO::new(&mut child_buf);

    let mut child_tree = FsNode::new_container(alloc::vec![file(
        "greeting.txt",
        b"Hello from nested lazy ISO!",
    )]);

    let mut child_injector = IsoInjector::new(&mut child_io, &child_meta).unwrap();
    child_injector.inject_tree(&mut child_tree).unwrap();

    let parent_meta = IsoMeta::default();
    let mut parent_buf = alloc::vec![0u8; 300 * ISO_SECTOR_SIZE];
    let mut parent_io = MemRimIO::new(&mut parent_buf);

    let mut parent_tree = FsNode::Container {
        attr: FileAttributes::new_dir(),
        children: alloc::vec![FsNode::new_file_from_source(
            "ISOS/nested.iso",
            alloc::boxed::Box::new(SliceRimIO::new(&child_buf)),
            FileAttributes::new_file(),
        ),],
    };

    let mut parent_injector = IsoInjector::new(&mut parent_io, &parent_meta).unwrap();
    parent_injector.inject_tree(&mut parent_tree).unwrap();

    let total_bytes = Arc::new(AtomicUsize::new(0));
    let total_reads = Arc::new(AtomicUsize::new(0));
    let mut counting_io = CountingRimIO::new(
        SliceRimIO::new(&parent_buf),
        total_bytes.clone(),
        total_reads,
    );

    let mut parent_resolver = IsoResolver::new(&mut counting_io, &parent_meta);
    assert!(parent_resolver.exists("ISOS/nested.iso"));

    let bytes_before_open = total_bytes.load(Ordering::SeqCst);
    let mut nested_file = parent_resolver.open_file("ISOS/nested.iso").unwrap();
    assert_eq!(nested_file.total_size().unwrap(), child_buf.len() as u64);

    // Opening nested.iso must not eagerly read the 200 KiB payload.
    assert_eq!(total_bytes.load(Ordering::SeqCst), bytes_before_open);

    let mut child_resolver = IsoResolver::new(&mut *nested_file, &child_meta);
    assert!(child_resolver.exists("greeting.txt"));

    let mut greeting_stream = child_resolver.open_file("greeting.txt").unwrap();
    assert_eq!(greeting_stream.total_size().unwrap(), 27);

    let mut greeting_buf = [0u8; 27];
    greeting_stream.read_at(0, &mut greeting_buf).unwrap();
    assert_eq!(&greeting_buf, b"Hello from nested lazy ISO!");

    assert!(total_bytes.load(Ordering::SeqCst) < 10 * ISO_SECTOR_SIZE);
}

#[test]
fn test_iso_fallible_try_new_and_unsupported_incremental_mutation() {
    let meta = IsoMeta::default();
    let mut empty_buf = [0u8; 100 * ISO_SECTOR_SIZE];
    let mut io = MemRimIO::new(&mut empty_buf);

    // try_new should fail on an unformatted ISO image
    let res = IsoResolver::try_new(&mut io, &meta);
    assert!(res.is_err());

    // write_file / write_dir / set_root_context must return Unsupported
    let mut injector = IsoInjector::new(&mut io, &meta).unwrap();
    let file_attrs = rimfs_core::resolver::attr::FileAttributes::new_file();
    let mut dummy = rimio::SliceRimIO::new(b"data");
    assert!(matches!(
        injector.write_file("file.txt", &mut dummy, 4, &file_attrs),
        Err(rimfs_core::FsInjectorError::Unsupported(_))
    ));
    assert!(matches!(
        injector.write_dir("dir", &file_attrs),
        Err(rimfs_core::FsInjectorError::Unsupported(_))
    ));
    assert!(matches!(
        injector.set_root_context(&file_attrs),
        Err(rimfs_core::FsInjectorError::Unsupported(_))
    ));
}
