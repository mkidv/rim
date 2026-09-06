use rimfs::core::checker::{FsChecker, VerifyReport};
use rimfs::core::formatter::FsFormatter;
use rimfs::core::injector::FsTreeInjector;
use rimfs::core::resolver::attr::FileAttributes;
use rimfs::core::resolver::node::FsNode;
use rimfs::core::resolver::{FsNodeCounts, FsTreeResolver};
use rimfs::{exfat, ext, fat, iso, ntfs, tar, zip};
use rimio::{MemRimIO, RimRead};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Engine {
    Fat,
    ExFat,
    Ext,
    Ntfs,
    Tar,
    Zip,
    Iso,
}

struct Image {
    engine: Engine,
    bytes: Vec<u8>,
}

#[derive(Clone, Copy)]
struct ExpectedFile {
    path: &'static str,
    bytes: &'static [u8],
}

const SMALL_FILES: &[ExpectedFile] = &[
    ExpectedFile {
        path: "empty.bin",
        bytes: b"",
    },
    ExpectedFile {
        path: "hello.txt",
        bytes: b"hello from cross-fs transfer\n",
    },
    ExpectedFile {
        path: "nested/deep/config.toml",
        bytes: b"[rim]\ntransfer = true\n",
    },
    ExpectedFile {
        path: "unicode/ete-café.txt",
        bytes: "données accentuées\n".as_bytes(),
    },
];

const DIRS: &[&str] = &["nested", "nested/deep", "unicode"];
const LARGE_FILE: &str = "large/stream.bin";
const LARGE_SIZE: usize = 2 * 1024 * 1024 + 333;

fn portable_tree() -> FsNode<'static> {
    FsNode::new_container(vec![
        file_with_attr("empty.bin", b"", portable_file_attr()),
        file_with_attr(
            "hello.txt",
            b"hello from cross-fs transfer\n",
            portable_file_attr(),
        ),
        FsNode::Dir {
            name: "nested".to_string(),
            attr: portable_dir_attr(),
            children: vec![FsNode::Dir {
                name: "deep".to_string(),
                attr: portable_dir_attr(),
                children: vec![file_with_attr(
                    "config.toml",
                    b"[rim]\ntransfer = true\n",
                    portable_file_attr(),
                )],
            }],
        },
        FsNode::Dir {
            name: "unicode".to_string(),
            attr: portable_dir_attr(),
            children: vec![file_with_attr(
                "ete-café.txt",
                "données accentuées\n".as_bytes(),
                portable_file_attr(),
            )],
        },
        FsNode::Dir {
            name: "large".to_string(),
            attr: portable_dir_attr(),
            children: vec![FsNode::new_file("stream.bin", large_pattern(LARGE_SIZE))],
        },
    ])
}

fn symlink_tree() -> FsNode<'static> {
    let mut link_attr = FileAttributes::new_symlink();
    link_attr.mode = Some(0o120777);
    link_attr.uid = Some(1000);
    link_attr.gid = Some(1000);
    link_attr.modified = timestamp();

    FsNode::new_container(vec![
        file_with_attr("target.txt", b"symlink target\n", posix_file_attr()),
        FsNode::Symlink {
            name: "target.link".to_string(),
            target: "target.txt".to_string(),
            attr: link_attr,
        },
    ])
}

fn file_with_attr(name: &str, bytes: &[u8], attr: FileAttributes) -> FsNode<'static> {
    FsNode::new_file_from_source(name, Box::new(rimio::VecRimIO::new(bytes.to_vec())), attr)
}

fn portable_file_attr() -> FileAttributes {
    let mut attr = FileAttributes::new_file();
    attr.read_only = true;
    attr.archive = true;
    attr.modified = timestamp();
    attr.mode = Some(0o100644);
    attr.uid = Some(1000);
    attr.gid = Some(1000);
    attr
}

fn portable_dir_attr() -> FileAttributes {
    let mut attr = FileAttributes::new_dir();
    attr.modified = timestamp();
    attr.mode = Some(0o040755);
    attr.uid = Some(1000);
    attr.gid = Some(1000);
    attr
}

fn posix_file_attr() -> FileAttributes {
    let mut attr = portable_file_attr();
    attr.mode = Some(0o100640);
    attr
}

fn timestamp() -> Option<rimfs::core::time::OffsetDateTime> {
    rimfs::core::time::OffsetDateTime::from_unix_timestamp(1_704_067_200).ok()
}

fn large_pattern(len: usize) -> Vec<u8> {
    (0..len)
        .map(|idx| ((idx.wrapping_mul(31) ^ (idx >> 3)) & 0xff) as u8)
        .collect()
}

fn image_size(engine: Engine) -> usize {
    match engine {
        Engine::Fat => 96 * 1024 * 1024,
        Engine::ExFat => 96 * 1024 * 1024,
        Engine::Ext => 96 * 1024 * 1024,
        Engine::Ntfs => 128 * 1024 * 1024,
        Engine::Tar => 8 * 1024 * 1024,
        Engine::Zip => 8 * 1024 * 1024,
        Engine::Iso => 32 * 1024 * 1024,
    }
}

fn build_image(engine: Engine, mut tree: FsNode<'static>) -> Image {
    let mut bytes = vec![0u8; image_size(engine)];
    match engine {
        Engine::Fat => {
            let meta = fat::FatMeta::new_fat32(bytes.len() as u64, Some("SRC")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            fat::FatFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            fat::FatInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree(&mut tree)
                .unwrap();
        }
        Engine::ExFat => {
            let meta = exfat::ExFatMeta::new(bytes.len() as u64, Some("SRC")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            exfat::ExFatFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            exfat::ExFatInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree(&mut tree)
                .unwrap();
        }
        Engine::Ext => {
            let meta = ext::ExtMeta::new(bytes.len() as u64, Some("SRC")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            ext::ExtFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            ext::ExtInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree(&mut tree)
                .unwrap();
        }
        Engine::Ntfs => {
            let meta = ntfs::NtfsMeta::new(bytes.len() as u64, Some("SRC")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            ntfs::NtfsFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            ntfs::NtfsInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree(&mut tree)
                .unwrap();
        }
        Engine::Tar => {
            let meta = tar::TarMeta::new(bytes.len() as u64, Some("SRC")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            tar::TarFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            tar::TarInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree(&mut tree)
                .unwrap();
        }
        Engine::Zip => {
            let meta = zip::ZipMeta::new(bytes.len() as u64, Some("SRC")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            zip::ZipFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            zip::ZipInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree(&mut tree)
                .unwrap();
        }
        Engine::Iso => {
            let meta = iso::IsoMeta::new(bytes.len() as u64, Some("SRC")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            iso::IsoFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            iso::IsoInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree(&mut tree)
                .unwrap();
        }
    }
    Image { engine, bytes }
}

fn transfer_to_dest(source: &mut Image, dest_engine: Engine) -> Image {
    match source.engine {
        Engine::Fat => {
            let meta = fat::FatMeta::from_io(&mut MemRimIO::new(&mut source.bytes)).unwrap();
            let mut src_io = MemRimIO::new(&mut source.bytes);
            let mut resolver = fat::FatResolver::new(&mut src_io, &meta);
            inject_from_resolver(source.engine, &mut resolver, dest_engine).0
        }
        Engine::ExFat => {
            let meta = exfat::ExFatMeta::from_io(&mut MemRimIO::new(&mut source.bytes)).unwrap();
            let mut src_io = MemRimIO::new(&mut source.bytes);
            let mut resolver = exfat::ExFatResolver::new(&mut src_io, &meta);
            inject_from_resolver(source.engine, &mut resolver, dest_engine).0
        }
        Engine::Ext => {
            let meta = ext::ExtMeta::from_io(&mut MemRimIO::new(&mut source.bytes)).unwrap();
            let mut src_io = MemRimIO::new(&mut source.bytes);
            let mut resolver = ext::ExtResolver::new(&mut src_io, &meta);
            inject_from_resolver(source.engine, &mut resolver, dest_engine).0
        }
        Engine::Ntfs => {
            let meta = ntfs::NtfsMeta::from_io(&mut MemRimIO::new(&mut source.bytes)).unwrap();
            let mut src_io = MemRimIO::new(&mut source.bytes);
            let mut resolver = ntfs::NtfsResolver::new(&mut src_io, &meta);
            inject_from_resolver(source.engine, &mut resolver, dest_engine).0
        }
        Engine::Tar => {
            let meta = tar::TarMeta::new(source.bytes.len() as u64, Some("SRC")).unwrap();
            let mut src_io = MemRimIO::new(&mut source.bytes);
            let mut resolver = tar::TarResolver::new(&mut src_io, &meta);
            inject_from_resolver(source.engine, &mut resolver, dest_engine).0
        }
        Engine::Zip => {
            let meta = zip::ZipMeta::new(source.bytes.len() as u64, Some("SRC")).unwrap();
            let mut src_io = MemRimIO::new(&mut source.bytes);
            let mut resolver = zip::ZipResolver::new(&mut src_io, &meta);
            inject_from_resolver(source.engine, &mut resolver, dest_engine).0
        }
        Engine::Iso => {
            let meta = iso::IsoMeta::new(source.bytes.len() as u64, Some("SRC")).unwrap();
            let mut src_io = MemRimIO::new(&mut source.bytes);
            let mut resolver = iso::IsoResolver::new(&mut src_io, &meta);
            inject_from_resolver(source.engine, &mut resolver, dest_engine).0
        }
    }
}

fn inject_from_resolver(
    source_engine: Engine,
    resolver: &mut dyn FsTreeResolver,
    dest_engine: Engine,
) -> (Image, FsNodeCounts) {
    let mut bytes = vec![0u8; image_size(dest_engine)];
    let counts = match dest_engine {
        Engine::Fat => {
            let meta = fat::FatMeta::new_fat32(bytes.len() as u64, Some("DST")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            fat::FatFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            fat::FatInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree_from_resolver(resolver, "/*")
        }
        Engine::ExFat => {
            let meta = exfat::ExFatMeta::new(bytes.len() as u64, Some("DST")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            exfat::ExFatFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            exfat::ExFatInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree_from_resolver(resolver, "/*")
        }
        Engine::Ext => {
            let meta = ext::ExtMeta::new(bytes.len() as u64, Some("DST")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            ext::ExtFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            ext::ExtInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree_from_resolver(resolver, "/*")
        }
        Engine::Ntfs => {
            let meta = ntfs::NtfsMeta::new(bytes.len() as u64, Some("DST")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            ntfs::NtfsFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            ntfs::NtfsInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree_from_resolver(resolver, "/*")
        }
        Engine::Tar => {
            let meta = tar::TarMeta::new(bytes.len() as u64, Some("DST")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            tar::TarFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            tar::TarInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree_from_resolver(resolver, "/*")
        }
        Engine::Zip => {
            let meta = zip::ZipMeta::new(bytes.len() as u64, Some("DST")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            zip::ZipFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            zip::ZipInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree_from_resolver(resolver, "/*")
        }
        Engine::Iso => {
            let meta = iso::IsoMeta::new(bytes.len() as u64, Some("DST")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            iso::IsoFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            iso::IsoInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree_from_resolver(resolver, "/*")
        }
    }
    .unwrap_or_else(|err| {
        panic!("{source_engine:?}->{dest_engine:?}: failed to transfer: {err:?}")
    });

    (
        Image {
            engine: dest_engine,
            bytes,
        },
        counts,
    )
}

fn assert_destination(source_engine: Engine, image: &mut Image) {
    let dest_engine = image.engine;
    with_resolver(image, |resolver| {
        for dir in DIRS {
            let attr = resolver.read_attributes(dir).unwrap_or_else(|err| {
                let parent = dir.rsplit_once('/').map(|(parent, _)| parent).unwrap_or("");
                let entries = resolver.read_dir(parent).unwrap_or_default();
                panic!(
                    "{source_engine:?}->{dest_engine:?}: missing directory {dir}: {err:?}; parent entries: {entries:?}"
                )
            });
            assert!(
                attr.is_dir(),
                "{source_engine:?}->{dest_engine:?}: expected directory {dir}"
            );
        }
        for file in SMALL_FILES {
            let bytes = resolver.read_file(file.path).unwrap_or_else(|err| {
                panic!(
                    "{source_engine:?}->{dest_engine:?}: failed to read {}: {err:?}",
                    file.path
                )
            });
            assert_eq!(
                bytes, file.bytes,
                "{source_engine:?}->{dest_engine:?}: content mismatch for {}",
                file.path
            );
        }
        assert_large_file(source_engine, dest_engine, resolver);
    });
    assert_checker_clean(image);
}

fn assert_large_file(
    source_engine: Engine,
    dest_engine: Engine,
    resolver: &mut dyn FsTreeResolver,
) {
    let mut file = resolver.open_file(LARGE_FILE).unwrap_or_else(|err| {
        panic!("{source_engine:?}->{dest_engine:?}: failed to open {LARGE_FILE}: {err:?}")
    });
    assert_eq!(
        file.total_size().unwrap(),
        LARGE_SIZE as u64,
        "{source_engine:?}->{dest_engine:?}: large file size mismatch"
    );

    let mut buf = [0u8; 8192];
    let mut offset = 0u64;
    while offset < LARGE_SIZE as u64 {
        let len = (LARGE_SIZE as u64 - offset).min(buf.len() as u64) as usize;
        file.read_at(offset, &mut buf[..len]).unwrap();
        for (idx, actual) in buf[..len].iter().enumerate() {
            let abs = offset as usize + idx;
            let expected = ((abs.wrapping_mul(31) ^ (abs >> 3)) & 0xff) as u8;
            assert_eq!(
                *actual, expected,
                "{source_engine:?}->{dest_engine:?}: large file mismatch at byte {abs}"
            );
        }
        offset += len as u64;
    }
}

fn with_resolver(image: &mut Image, check: impl FnOnce(&mut dyn FsTreeResolver)) {
    match image.engine {
        Engine::Fat => {
            let meta = fat::FatMeta::from_io(&mut MemRimIO::new(&mut image.bytes)).unwrap();
            let mut io = MemRimIO::new(&mut image.bytes);
            let mut resolver = fat::FatResolver::new(&mut io, &meta);
            check(&mut resolver);
        }
        Engine::ExFat => {
            let meta = exfat::ExFatMeta::from_io(&mut MemRimIO::new(&mut image.bytes)).unwrap();
            let mut io = MemRimIO::new(&mut image.bytes);
            let mut resolver = exfat::ExFatResolver::new(&mut io, &meta);
            check(&mut resolver);
        }
        Engine::Ext => {
            let meta = ext::ExtMeta::from_io(&mut MemRimIO::new(&mut image.bytes)).unwrap();
            let mut io = MemRimIO::new(&mut image.bytes);
            let mut resolver = ext::ExtResolver::new(&mut io, &meta);
            check(&mut resolver);
        }
        Engine::Ntfs => {
            let meta = ntfs::NtfsMeta::from_io(&mut MemRimIO::new(&mut image.bytes)).unwrap();
            let mut io = MemRimIO::new(&mut image.bytes);
            let mut resolver = ntfs::NtfsResolver::new(&mut io, &meta);
            check(&mut resolver);
        }
        Engine::Tar => {
            let meta = tar::TarMeta::new(image.bytes.len() as u64, None).unwrap();
            let mut io = MemRimIO::new(&mut image.bytes);
            let mut resolver = tar::TarResolver::new(&mut io, &meta);
            check(&mut resolver);
        }
        Engine::Zip => {
            let meta = zip::ZipMeta::new(image.bytes.len() as u64, None).unwrap();
            let mut io = MemRimIO::new(&mut image.bytes);
            let mut resolver = zip::ZipResolver::new(&mut io, &meta);
            check(&mut resolver);
        }
        Engine::Iso => {
            let meta = iso::IsoMeta::new(image.bytes.len() as u64, None).unwrap();
            let mut io = MemRimIO::new(&mut image.bytes);
            let mut resolver = iso::IsoResolver::new(&mut io, &meta);
            check(&mut resolver);
        }
    }
}

fn assert_checker_clean(image: &mut Image) {
    let report = match image.engine {
        Engine::Fat => {
            let meta = fat::FatMeta::from_io(&mut MemRimIO::new(&mut image.bytes)).unwrap();
            let mut io = MemRimIO::new(&mut image.bytes);
            fat::FatChecker::new(&mut io, &meta).check_all().unwrap()
        }
        Engine::ExFat => {
            let meta = exfat::ExFatMeta::from_io(&mut MemRimIO::new(&mut image.bytes)).unwrap();
            let mut io = MemRimIO::new(&mut image.bytes);
            exfat::ExFatChecker::new(&mut io, &meta)
                .check_all()
                .unwrap()
        }
        Engine::Ext => {
            let meta = ext::ExtMeta::from_io(&mut MemRimIO::new(&mut image.bytes)).unwrap();
            let mut io = MemRimIO::new(&mut image.bytes);
            ext::ExtChecker::new(&mut io, &meta).check_all().unwrap()
        }
        Engine::Ntfs => {
            let meta = ntfs::NtfsMeta::from_io(&mut MemRimIO::new(&mut image.bytes)).unwrap();
            let mut io = MemRimIO::new(&mut image.bytes);
            ntfs::NtfsChecker::new(&mut io, &meta).check_all().unwrap()
        }
        Engine::Tar => {
            let meta = tar::TarMeta::new(image.bytes.len() as u64, None).unwrap();
            let mut io = MemRimIO::new(&mut image.bytes);
            tar::TarChecker::new(&mut io, &meta).check_all().unwrap()
        }
        Engine::Zip => {
            let meta = zip::ZipMeta::new(image.bytes.len() as u64, None).unwrap();
            let mut io = MemRimIO::new(&mut image.bytes);
            zip::ZipChecker::new(&mut io, &meta).check_all().unwrap()
        }
        Engine::Iso => {
            let meta = iso::IsoMeta::new(image.bytes.len() as u64, None).unwrap();
            let mut io = MemRimIO::new(&mut image.bytes);
            iso::IsoChecker::new(&mut io, &meta).check_all().unwrap()
        }
    };
    assert_report_clean(&report, image.engine);
}

fn assert_report_clean(report: &VerifyReport, engine: Engine) {
    assert!(
        !report.has_error(),
        "{engine:?} checker reported {report:?}"
    );
}

#[test]
fn read_write_engines_transfer_portable_tree() {
    let engines = [Engine::Fat, Engine::ExFat, Engine::Ext, Engine::Ntfs];

    for source_engine in engines {
        for dest_engine in engines {
            let mut source = build_image(source_engine, portable_tree());
            let mut dest = transfer_to_dest(&mut source, dest_engine);
            assert_destination(source_engine, &mut dest);
        }
    }
}

#[test]
fn read_oriented_sources_transfer_to_read_write_engines() {
    let sources = [Engine::Tar, Engine::Zip, Engine::Iso];
    let destinations = [Engine::Fat, Engine::ExFat, Engine::Ext, Engine::Ntfs];

    for source_engine in sources {
        for dest_engine in destinations {
            let mut source = build_image(source_engine, portable_tree());
            let mut dest = transfer_to_dest(&mut source, dest_engine);
            assert_destination(source_engine, &mut dest);
        }
    }
}

#[test]
fn representable_symlinks_roundtrip_between_posix_like_engines() {
    let destinations = [Engine::Ext, Engine::Tar, Engine::Zip, Engine::Iso];

    for dest_engine in destinations {
        let mut source = build_image(Engine::Ext, symlink_tree());
        let mut dest = transfer_to_dest(&mut source, dest_engine);
        with_resolver(&mut dest, |resolver| {
            assert_eq!(
                resolver.read_file("target.txt").unwrap(),
                b"symlink target\n"
            );
            assert_eq!(resolver.read_link("target.link").unwrap(), "target.txt");
            let attr = resolver.read_attributes("target.link").unwrap();
            assert!(
                attr.is_symlink(),
                "{dest_engine:?} did not preserve symlink kind"
            );
        });
        assert_checker_clean(&mut dest);
    }
}

#[test]
fn unsupported_symlinks_fail_without_silent_loss() {
    let destinations = [Engine::Fat, Engine::ExFat, Engine::Ntfs];

    for dest_engine in destinations {
        let mut source = build_image(Engine::Ext, symlink_tree());
        match source.engine {
            Engine::Ext => {
                let meta = ext::ExtMeta::from_io(&mut MemRimIO::new(&mut source.bytes)).unwrap();
                let mut src_io = MemRimIO::new(&mut source.bytes);
                let mut resolver = ext::ExtResolver::new(&mut src_io, &meta);
                let mut bytes = vec![0u8; image_size(dest_engine)];
                let result = match dest_engine {
                    Engine::Fat => {
                        let meta =
                            fat::FatMeta::new_fat32(bytes.len() as u64, Some("DST")).unwrap();
                        let mut io = MemRimIO::new(&mut bytes);
                        fat::FatFormatter::new(&mut io, &meta)
                            .format(false)
                            .unwrap();
                        fat::FatInjector::new(&mut io, &meta)
                            .unwrap()
                            .inject_tree_from_resolver(&mut resolver, "/*")
                    }
                    Engine::ExFat => {
                        let meta = exfat::ExFatMeta::new(bytes.len() as u64, Some("DST")).unwrap();
                        let mut io = MemRimIO::new(&mut bytes);
                        exfat::ExFatFormatter::new(&mut io, &meta)
                            .format(false)
                            .unwrap();
                        exfat::ExFatInjector::new(&mut io, &meta)
                            .unwrap()
                            .inject_tree_from_resolver(&mut resolver, "/*")
                    }
                    Engine::Ntfs => {
                        let meta = ntfs::NtfsMeta::new(bytes.len() as u64, Some("DST")).unwrap();
                        let mut io = MemRimIO::new(&mut bytes);
                        ntfs::NtfsFormatter::new(&mut io, &meta)
                            .format(false)
                            .unwrap();
                        ntfs::NtfsInjector::new(&mut io, &meta)
                            .unwrap()
                            .inject_tree_from_resolver(&mut resolver, "/*")
                    }
                    _ => unreachable!(),
                };
                assert!(result.is_err(), "{dest_engine:?} silently accepted symlink");
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn posix_metadata_is_preserved_when_representable() {
    let destinations = [Engine::Ext, Engine::Tar, Engine::Zip, Engine::Iso];

    for dest_engine in destinations {
        let mut source = build_image(Engine::Ext, symlink_tree());
        let mut dest = transfer_to_dest(&mut source, dest_engine);
        with_resolver(&mut dest, |resolver| {
            let file_attr = resolver.read_attributes("target.txt").unwrap();
            assert_mode_bits(file_attr.mode, 0o640, "file", dest_engine);
            assert_eq!(
                file_attr.uid,
                Some(1000),
                "Ext->{dest_engine:?}: uid was not preserved"
            );
            assert_eq!(
                file_attr.gid,
                Some(1000),
                "Ext->{dest_engine:?}: gid was not preserved"
            );
            assert_eq!(
                file_attr.modified,
                timestamp(),
                "Ext->{dest_engine:?}: mtime was not preserved"
            );

            let link_attr = resolver.read_attributes("target.link").unwrap();
            assert_mode_bits(link_attr.mode, 0o777, "symlink", dest_engine);
            assert_eq!(
                link_attr.uid,
                Some(1000),
                "Ext->{dest_engine:?}: symlink uid was not preserved"
            );
            assert_eq!(
                link_attr.gid,
                Some(1000),
                "Ext->{dest_engine:?}: symlink gid was not preserved"
            );
        });
    }
}

fn assert_mode_bits(actual: Option<u32>, expected: u32, kind: &str, dest_engine: Engine) {
    assert_eq!(
        actual.map(|mode| mode & 0o7777),
        Some(expected),
        "Ext->{dest_engine:?}: {kind} mode bits were not preserved"
    );
}
