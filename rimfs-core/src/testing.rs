// SPDX-License-Identifier: MIT

//! Shared testing utilities and report assertion helpers.

extern crate alloc;

use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use crate::{
    checker::{FsChecker, Severity, VerifyReport},
    filesystem::FsFilesystem,
    formatter::FsFormatter,
    injector::FsTreeInjector,
    resolver::{FileAttributes, FsNode, FsTreeResolver},
};
use rimio::{RimIO, prelude::VecRimIO};

pub struct ExpectedFile<'a> {
    pub path: &'a str,
    pub bytes: &'a [u8],
}

pub struct ExpectedLink<'a> {
    pub path: &'a str,
    pub target: &'a str,
}

pub fn file<'a>(name: impl Into<String>, bytes: &'a [u8]) -> FsNode<'a> {
    file_with_attr(name, bytes, FileAttributes::new_file())
}

pub fn file_with_attr<'a>(
    name: impl Into<String>,
    bytes: &'a [u8],
    attr: FileAttributes,
) -> FsNode<'a> {
    FsNode::new_file_from_source(name, Box::new(VecRimIO::new(bytes.to_vec())), attr)
}

pub fn basic_tree<'a>() -> FsNode<'a> {
    FsNode::new_container(vec![
        file("hello.txt", b"Hello World!"),
        FsNode::new_dir("subdir"),
        file("subdir/nested.txt", b"Nested file content"),
        FsNode::new_symlink("link_to_hello", "hello.txt"),
    ])
}

pub fn nested_files_tree<'a>() -> FsNode<'a> {
    FsNode::Container {
        attr: FileAttributes::new_dir(),
        children: vec![
            FsNode::Dir {
                name: "subdir".to_string(),
                attr: FileAttributes::new_dir(),
                children: vec![FsNode::new_file("hello.txt", b"Hello World!".to_vec())],
            },
            FsNode::new_file("readme.md", b"Test Readme".to_vec()),
        ],
    }
}

pub fn assert_structural_tree_eq(expected: &mut FsNode<'_>, actual: &mut FsNode<'_>, name: &str) {
    expected.sort_children_recursively();
    actual.sort_children_recursively();

    assert!(
        expected.structural_eq(actual),
        "Tree structure mismatch for {name}\nExpected:\n{expected}\nActual:\n{actual}"
    );
}

pub fn assert_no_findings(report: &VerifyReport) {
    assert_eq!(report.findings.len(), 0, "{report:?}");
}

pub fn assert_no_errors(report: &VerifyReport) {
    assert!(!report.has_error(), "{report:?}");
}

pub fn assert_has_error(report: &VerifyReport, code: &str) {
    expect_error(report, code);
}

/// Find an error by stable diagnostic code, preserving the full report on failure.
pub fn expect_error<'a>(report: &'a VerifyReport, code: &str) -> &'a crate::checker::Finding {
    report
        .findings
        .iter()
        .find(|finding| finding.code == code && finding.sev == Severity::Error)
        .unwrap_or_else(|| panic!("expected error {code}, got {report:?}"))
}

pub fn assert_has_warning(report: &VerifyReport, code: &str) {
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == code && finding.sev == Severity::Warn),
        "expected warning {code}, got {report:?}"
    );
}

pub fn assert_exists<R: FsTreeResolver>(resolver: &mut R, paths: &[&str]) {
    for path in paths {
        assert!(resolver.exists(path), "expected path to exist: {path}");
    }
}

pub fn assert_missing<R: FsTreeResolver>(resolver: &mut R, paths: &[&str]) {
    for path in paths {
        assert!(
            !resolver.exists(path),
            "expected path to be missing: {path}"
        );
    }
}

pub fn assert_dirs<R: FsTreeResolver>(resolver: &mut R, paths: &[&str]) {
    for path in paths {
        let attr = resolver
            .read_attributes(path)
            .unwrap_or_else(|err| panic!("failed to read attributes for {path}: {err:?}"));
        assert!(attr.is_dir(), "expected directory: {path}");
    }
}

pub fn assert_files<R: FsTreeResolver>(resolver: &mut R, files: &[ExpectedFile<'_>]) {
    for file in files {
        let bytes = resolver
            .read_file(file.path)
            .unwrap_or_else(|err| panic!("failed to read file {}: {err:?}", file.path));
        assert_eq!(bytes, file.bytes, "unexpected file content: {}", file.path);
    }
}

pub fn assert_symlinks<R: FsTreeResolver>(resolver: &mut R, links: &[ExpectedLink<'_>]) {
    for link in links {
        let target = resolver
            .read_link(link.path)
            .unwrap_or_else(|err| panic!("failed to read link {}: {err:?}", link.path));
        assert_eq!(target, link.target, "unexpected link target: {}", link.path);
    }
}

pub fn assert_dir_entries<R: FsTreeResolver>(resolver: &mut R, path: &str, expected: &[&str]) {
    let mut entries = resolver
        .read_dir(path)
        .unwrap_or_else(|err| panic!("failed to read directory {path}: {err:?}"));
    entries.sort();

    let mut expected: Vec<String> = expected.iter().map(|entry| entry.to_string()).collect();
    expected.sort();

    assert_eq!(entries, expected, "unexpected directory entries: {path}");
}

/// Deterministic, fail-before-operation I/O adapter for mutation tests.
/// The zero-based operation index counts reads, writes and flushes. A fault fires
/// once; subsequent I/O succeeds normally. This does not simulate torn writes or
/// durable storage. Offset and size queries delegate without consuming an index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultOperation {
    Read { offset: u64, len: usize },
    Write { offset: u64, len: usize },
    Flush,
}

pub struct FaultRimIO<IO> {
    pub inner: IO,
    fail_at: Option<usize>,
    persistent: bool,
    partial_write_len: Option<usize>,
    pub operations: Vec<FaultOperation>,
    pub triggered: bool,
}

impl<IO> FaultRimIO<IO> {
    pub fn new(inner: IO, fail_at: Option<usize>) -> Self {
        Self {
            inner,
            fail_at,
            persistent: false,
            partial_write_len: None,
            operations: Vec::new(),
            triggered: false,
        }
    }

    pub fn with_persistent(mut self, persistent: bool) -> Self {
        self.persistent = persistent;
        self
    }

    pub fn with_partial_write(mut self, partial_len: usize) -> Self {
        self.partial_write_len = Some(partial_len);
        self
    }
}

impl<IO: rimio::RimRead> rimio::RimRead for FaultRimIO<IO> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> rimio::RimIOResult {
        let index = self.operations.len();
        self.operations.push(FaultOperation::Read {
            offset,
            len: buf.len(),
        });
        if self.fail_at == Some(index) || (self.persistent && self.triggered) {
            self.triggered = true;
            return Err(rimio::RimIOError::Other("Injected I/O failure"));
        }
        self.inner.read_at(offset, buf)
    }
    fn total_size(&mut self) -> rimio::RimIOResult<u64> {
        self.inner.total_size()
    }
}
impl<IO: rimio::RimWrite> rimio::RimWrite for FaultRimIO<IO> {
    fn write_at(&mut self, offset: u64, data: &[u8]) -> rimio::RimIOResult {
        let index = self.operations.len();
        self.operations.push(FaultOperation::Write {
            offset,
            len: data.len(),
        });
        if self.fail_at == Some(index) || (self.persistent && self.triggered) {
            self.triggered = true;
            if let Some(partial_len) = self.partial_write_len {
                let to_write = partial_len.min(data.len());
                if to_write > 0 {
                    let _ = self.inner.write_at(offset, &data[..to_write]);
                }
            }
            return Err(rimio::RimIOError::Other("Injected I/O failure"));
        }
        self.inner.write_at(offset, data)
    }
    fn flush(&mut self) -> rimio::RimIOResult {
        let index = self.operations.len();
        self.operations.push(FaultOperation::Flush);
        if self.fail_at == Some(index) || (self.persistent && self.triggered) {
            self.triggered = true;
            return Err(rimio::RimIOError::Other("Injected I/O failure"));
        }
        self.inner.flush()
    }
}
impl<IO: rimio::RimIO> rimio::RimIO for FaultRimIO<IO> {
    fn set_offset(&mut self, offset: u64) -> u64 {
        self.inner.set_offset(offset)
    }
    fn partition_offset(&self) -> u64 {
        self.inner.partition_offset()
    }
}

/// An I/O wrapper that injects failures at targeted offsets or conditions.
pub struct FailingRimIO<IO> {
    pub inner: IO,
    fail_read_offset: Option<u64>,
    fail_write_offset: Option<u64>,
    persistent: bool,
    pub triggered: bool,
}

impl<IO> FailingRimIO<IO> {
    pub fn new(inner: IO) -> Self {
        Self {
            inner,
            fail_read_offset: None,
            fail_write_offset: None,
            persistent: false,
            triggered: false,
        }
    }

    pub fn fail_read_at(mut self, offset: u64) -> Self {
        self.fail_read_offset = Some(offset);
        self
    }

    pub fn fail_write_at(mut self, offset: u64) -> Self {
        self.fail_write_offset = Some(offset);
        self
    }

    pub fn with_persistent(mut self, persistent: bool) -> Self {
        self.persistent = persistent;
        self
    }
}

impl<IO: rimio::RimRead> rimio::RimRead for FailingRimIO<IO> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> rimio::RimIOResult {
        if (self.persistent && self.triggered) || self.fail_read_offset == Some(offset) {
            self.triggered = true;
            return Err(rimio::RimIOError::Other("Simulated I/O read failure"));
        }
        self.inner.read_at(offset, buf)
    }

    fn total_size(&mut self) -> rimio::RimIOResult<u64> {
        self.inner.total_size()
    }
}

impl<IO: rimio::RimWrite> rimio::RimWrite for FailingRimIO<IO> {
    fn write_at(&mut self, offset: u64, data: &[u8]) -> rimio::RimIOResult {
        if (self.persistent && self.triggered) || self.fail_write_offset == Some(offset) {
            self.triggered = true;
            return Err(rimio::RimIOError::Other("Simulated I/O write failure"));
        }
        self.inner.write_at(offset, data)
    }

    fn flush(&mut self) -> rimio::RimIOResult {
        if self.persistent && self.triggered {
            return Err(rimio::RimIOError::Other("Simulated I/O flush failure"));
        }
        self.inner.flush()
    }
}

impl<IO: rimio::RimIO> rimio::RimIO for FailingRimIO<IO> {
    fn set_offset(&mut self, offset: u64) -> u64 {
        self.inner.set_offset(offset)
    }

    fn partition_offset(&self) -> u64 {
        self.inner.partition_offset()
    }
}

/// Declares feature capabilities of a filesystem engine for conformance testing.
#[derive(Debug, Clone, Copy)]
pub struct FsCapabilities {
    pub supports_symlinks: bool,
    pub supports_nested_dirs: bool,
    pub case_sensitive: bool,
    pub max_file_size: u64,
    /// Allocation or record boundary supplied by the fixture; None for unaligned formats.
    pub boundary_size: Option<usize>,
    /// Exact freshly formatted root listing, when the format specifies it.
    pub formatted_root_entries: Option<&'static [&'static str]>,
}

impl Default for FsCapabilities {
    fn default() -> Self {
        Self {
            supports_symlinks: false,
            supports_nested_dirs: true,
            case_sensitive: false,
            max_file_size: u64::MAX,
            boundary_size: None,
            formatted_root_entries: None,
        }
    }
}

fn boundary_cases(boundary: usize) -> [(&'static str, usize); 4] {
    assert!(boundary > 0, "fixture boundary must be nonzero");
    let last = boundary
        .checked_mul(2)
        .and_then(|n| n.checked_add(1))
        .expect("fixture boundary overflow");
    [
        ("below.bin", boundary - 1),
        ("exact.bin", boundary),
        ("above.bin", boundary + 1),
        ("twoplus.bin", last),
    ]
}

fn boundary_payload(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 251) as u8).collect()
}

/// Generates a standardized directory tree testing boundary file sizes and hierarchies.
pub fn compliance_tree<'a>(caps: &FsCapabilities) -> FsNode<'a> {
    let mut children = vec![file("empty.txt", b""), file("byte.bin", b"A")];

    // Common sector size; the engine geometry may differ.
    let sector_data: Vec<u8> = (0..512).map(|i| (i % 251) as u8).collect();
    children.push(FsNode::new_file("sector.bin", sector_data));

    // Common block size; the engine geometry may differ.
    let cluster_data: Vec<u8> = (0..4096).map(|i| (i % 251) as u8).collect();
    children.push(FsNode::new_file("cluster.bin", cluster_data));

    // Multi-cluster payload (16 KiB = 16384 bytes)
    let multi_data: Vec<u8> = (0..16384).map(|i| (i % 251) as u8).collect();
    children.push(FsNode::new_file("large.bin", multi_data));

    if let Some(boundary) = caps.boundary_size {
        for (name, size) in boundary_cases(boundary) {
            assert!(
                size as u64 <= caps.max_file_size,
                "fixture exceeds file size limit"
            );
            children.push(FsNode::new_file(name, boundary_payload(size)));
        }
    }

    if caps.supports_nested_dirs {
        children.push(FsNode::Dir {
            name: "sub1".to_string(),
            attr: FileAttributes::new_dir(),
            children: vec![FsNode::Dir {
                name: "sub2".to_string(),
                attr: FileAttributes::new_dir(),
                children: vec![file("nested.txt", b"deep payload")],
            }],
        });
    }

    if caps.supports_symlinks {
        children.push(FsNode::new_symlink("symlink_to_byte", "byte.bin"));
    }

    FsNode::Container {
        attr: FileAttributes::new_dir(),
        children,
    }
}

/// Checks the exact-read contract through the stream exposed by a resolver.
/// Reads backwards and across common sector/block boundaries to catch readers
/// that accidentally depend on a cursor or expose allocation padding as data.
fn assert_compliance_stream<R: FsTreeResolver>(
    resolver: &mut R,
    path: &str,
    expected: &[u8],
    boundary: usize,
) {
    let mut stream = resolver
        .open_file(path)
        .expect("open compliance file failed");
    let size = expected.len() as u64;
    assert_eq!(
        stream.total_size().expect("stream size failed"),
        size,
        "{path}"
    );
    let mut buf = [0u8; 37];
    for offset in [
        boundary.saturating_mul(2).saturating_sub(1),
        boundary.saturating_sub(1),
        1,
        0,
        expected.len().saturating_sub(1),
    ] {
        if offset < expected.len() {
            let len = buf.len().min(expected.len() - offset);
            stream
                .read_at(offset as u64, &mut buf[..len])
                .expect("partial read failed");
            assert_eq!(
                &buf[..len],
                &expected[offset..offset + len],
                "{path} at {offset}"
            );
        }
    }
    stream
        .read_at(size, &mut [])
        .expect("empty read at EOF failed");
    assert!(
        stream.read_at(size, &mut buf[..1]).is_err(),
        "{path}: read past EOF succeeded"
    );
    if size > 0 {
        assert!(
            stream.read_at(size - 1, &mut buf[..2]).is_err(),
            "{path}: read crossing EOF succeeded"
        );
    }
}

/// Validates that all items in the compliance tree were correctly injected and can be read back byte-for-byte.
pub fn assert_compliance_readback<R: FsTreeResolver>(resolver: &mut R, caps: &FsCapabilities) {
    assert!(resolver.exists("/"), "root '/' must exist");

    // 1. Empty file
    assert!(resolver.exists("empty.txt"), "empty.txt must exist");
    let empty = resolver
        .read_file("empty.txt")
        .expect("read empty.txt failed");
    assert!(
        empty.is_empty(),
        "empty.txt must be 0 bytes, got {}",
        empty.len()
    );

    // 2. One-byte file
    assert!(resolver.exists("byte.bin"), "byte.bin must exist");
    let byte_data = resolver
        .read_file("byte.bin")
        .expect("read byte.bin failed");
    assert_eq!(byte_data, b"A", "byte.bin content mismatch");

    // 3. Sector boundary (512 bytes)
    let expected_sector: Vec<u8> = (0..512).map(|i| (i % 251) as u8).collect();
    let actual_sector = resolver
        .read_file("sector.bin")
        .expect("read sector.bin failed");
    assert_eq!(
        actual_sector, expected_sector,
        "sector.bin content mismatch"
    );

    // 4. Cluster boundary (4096 bytes)
    let expected_cluster: Vec<u8> = (0..4096).map(|i| (i % 251) as u8).collect();
    let actual_cluster = resolver
        .read_file("cluster.bin")
        .expect("read cluster.bin failed");
    assert_eq!(
        actual_cluster, expected_cluster,
        "cluster.bin content mismatch"
    );

    // 5. Multi-cluster (16 KiB)
    let expected_large: Vec<u8> = (0..16384).map(|i| (i % 251) as u8).collect();
    let actual_large = resolver
        .read_file("large.bin")
        .expect("read large.bin failed");
    assert_eq!(actual_large, expected_large, "large.bin content mismatch");

    for (path, expected) in [
        ("empty.txt", empty.as_slice()),
        ("byte.bin", byte_data.as_slice()),
        ("sector.bin", expected_sector.as_slice()),
        ("cluster.bin", expected_cluster.as_slice()),
        ("large.bin", expected_large.as_slice()),
    ] {
        assert!(resolver.is_file(path), "expected regular file: {path}");
        assert_compliance_stream(resolver, path, expected, caps.boundary_size.unwrap_or(4096));
    }
    if let Some(boundary) = caps.boundary_size {
        for (path, size) in boundary_cases(boundary) {
            let expected = boundary_payload(size);
            assert!(resolver.is_file(path), "expected regular file: {path}");
            assert_eq!(
                resolver.read_file(path).expect("boundary read failed"),
                expected,
                "{path}"
            );
            assert_compliance_stream(resolver, path, &expected, boundary);
        }
    }
    assert_eq!(
        resolver.exists("BYTE.BIN"),
        !caps.case_sensitive,
        "case lookup contract"
    );
    if !caps.case_sensitive {
        assert_eq!(resolver.read_file("BYTE.BIN").unwrap(), b"A");
    }
    let entries = resolver.read_dir("/").unwrap();
    assert!(
        entries.iter().any(|name| name == "byte.bin"),
        "listing must preserve names"
    );
    assert_dirs(resolver, &["/"]);
    assert_missing(resolver, &["missing-contract.bin"]);
    assert!(resolver.open_file("missing-contract.bin").is_err());

    // 6. Nested directories
    if caps.supports_nested_dirs {
        assert!(resolver.exists("sub1"), "sub1 must exist");
        assert!(resolver.exists("sub1/sub2"), "sub1/sub2 must exist");
        assert!(
            resolver.exists("sub1/sub2/nested.txt"),
            "nested.txt must exist"
        );
        assert_eq!(
            resolver.exists("SUB1/SUB2/NESTED.TXT"),
            !caps.case_sensitive
        );
        assert_eq!(
            resolver.read_file("/sub1/sub2/nested.txt").unwrap(),
            b"deep payload"
        );
        assert_dirs(resolver, &["sub1", "sub1/sub2"]);
        assert_dir_entries(resolver, "sub1", &["sub2"]);
        assert_dir_entries(resolver, "sub1/sub2", &["nested.txt"]);
        assert_missing(resolver, &["sub1/sub2/missing.txt"]);
        let nested = resolver
            .read_file("sub1/sub2/nested.txt")
            .expect("read nested.txt failed");
        assert_eq!(nested, b"deep payload", "nested.txt content mismatch");
    }

    // 7. Symlinks
    if caps.supports_symlinks {
        assert!(
            resolver.exists("symlink_to_byte"),
            "symlink_to_byte must exist"
        );
        let target = resolver
            .read_link("symlink_to_byte")
            .expect("read_link failed");
        assert_eq!(target, "byte.bin", "symlink target mismatch");
    }
}

/// Universal conformance test for any storage engine implementing `FsFilesystem`.
///
/// Validates:
/// 1. Formatter completes and flushes cleanly.
/// 2. Checker passes with 0 errors on virgin formatted filesystem.
/// 3. Root exists (native system entries are allowed).
/// 4. Fixed payloads and fixture boundary sizes B-1, B, B+1, 2B+1 inject cleanly.
/// 5. Checker passes with 0 errors post-injection.
/// 6. Resolver retrieves every payload byte-for-byte and verifies structural layout.
pub fn test_fs_contract_compliance<FS, M, IO>(io: &mut IO, meta: &M, caps: FsCapabilities)
where
    for<'a> FS: FsFilesystem<'a, Meta = M>,
    M: Clone,
    IO: RimIO,
{
    // Phase 1: Format
    {
        let mut fmt = FS::formatter(io, meta);
        fmt.format(false).expect("formatter failed");
        fmt.flush().expect("format flush failed");
    }

    // Phase 2: Post-format integrity check
    {
        let mut chk = FS::checker(io, meta);
        let rep = chk.check_all().expect("post-format checker failed");
        assert_no_errors(&rep);
        if caps.formatted_root_entries.is_some() {
            assert_no_findings(&rep);
        }
    }

    // Phase 3: Root invariant
    {
        let mut res = FS::resolver(io, meta);
        assert!(res.exists("/"), "root '/' must exist post-format");
        if let Some(entries) = caps.formatted_root_entries {
            assert_dir_entries(&mut res, "/", entries);
        }
    }

    // Phase 4: Inject compliance tree
    let mut tree = compliance_tree(&caps);
    {
        let mut inj = FS::injector(io, meta).expect("injector creation failed");
        inj.inject_tree(&mut tree).expect("injection failed");
        inj.flush().expect("flush failed");
    }

    // Phase 5: Post-injection integrity check
    {
        let mut chk = FS::checker(io, meta);
        let rep = chk.check_all().expect("post-injection checker failed");
        assert_no_errors(&rep);
    }

    // Phase 6: Resolver readback verification
    {
        let mut res = FS::resolver(io, meta);
        assert_compliance_readback(&mut res, &caps);
    }
    if caps.supports_nested_dirs {
        assert_contract_transfer::<FS, M, IO>(io, meta);
    }
}

/// Corrupt a single byte at the specified offset by applying a mutation function.
/// Returns the original byte before mutation.
pub fn corrupt_byte_at(
    io: &mut (dyn RimIO + '_),
    offset: u64,
    mutate: impl FnOnce(u8) -> u8,
) -> u8 {
    let mut buf = [0u8; 1];
    io.read_at(offset, &mut buf)
        .expect("read before corrupt failed");
    let original = buf[0];
    buf[0] = mutate(original);
    io.write_at(offset, &buf)
        .expect("write corrupt byte failed");
    original
}

/// Corrupt bytes at the specified offset with invalid replacement bytes.
/// Returns the original bytes before mutation.
pub fn corrupt_bytes_at(io: &mut (dyn RimIO + '_), offset: u64, bad_bytes: &[u8]) -> Vec<u8> {
    let mut original = vec![0u8; bad_bytes.len()];
    io.read_at(offset, &mut original)
        .expect("read before corrupt failed");
    io.write_at(offset, bad_bytes)
        .expect("write corrupt bytes failed");
    original
}

/// A source that requires the streaming API and can fail during payload reads.
struct ContractSource {
    fail: bool,
    triggered: core::cell::Cell<bool>,
}

struct ContractStream<'a> {
    source: &'a ContractSource,
}

impl rimio::RimRead for ContractStream<'_> {
    fn total_size(&mut self) -> rimio::RimIOResult<u64> {
        Ok(8193)
    }
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> rimio::RimIOResult {
        if self.source.fail {
            self.source.triggered.set(true);
            return Err(rimio::RimIOError::Other("contract source read failure"));
        }
        if offset
            .checked_add(buf.len() as u64)
            .is_none_or(|end| end > 8193)
        {
            return Err(rimio::RimIOError::OutOfBounds);
        }
        for (i, byte) in buf.iter_mut().enumerate() {
            *byte = ((offset + i as u64) % 251) as u8;
        }
        Ok(())
    }
}

impl FsTreeResolver for ContractSource {
    fn read_attributes(&mut self, path: &str) -> crate::resolver::FsResolverResult<FileAttributes> {
        match path.trim_matches('/') {
            "" | "transfer" => Ok(FileAttributes::new_dir()),
            "transfer/data.bin" => Ok(FileAttributes::new_file()),
            _ => Err(crate::resolver::FsResolverError::Invalid(
                "unknown contract path",
            )),
        }
    }
    fn read_dir(&mut self, path: &str) -> crate::resolver::FsResolverResult<Vec<String>> {
        match path.trim_matches('/') {
            "" => Ok(vec!["transfer".to_string()]),
            "transfer" => Ok(vec!["data.bin".to_string()]),
            _ => Err(crate::resolver::FsResolverError::Invalid(
                "unknown contract directory",
            )),
        }
    }
    fn open_file<'b>(
        &'b mut self,
        path: &str,
    ) -> crate::resolver::FsResolverResult<Box<dyn rimio::RimRead + 'b>> {
        assert_eq!(path.trim_matches('/'), "transfer/data.bin");
        Ok(Box::new(ContractStream { source: self }))
    }
    fn read_file(&mut self, _: &str) -> crate::resolver::FsResolverResult<Vec<u8>> {
        panic!("resolver transfer must use open_file, not materialize payloads")
    }
}

/// Exercises streaming transfer and source failure propagation on a fresh destination.
fn assert_contract_transfer<FS, M, IO>(io: &mut IO, meta: &M)
where
    for<'a> FS: FsFilesystem<'a, Meta = M>,
    M: Clone,
    IO: RimIO,
{
    let size = usize::try_from(io.total_size().unwrap()).expect("fixture too large");
    for fail in [false, true] {
        let mut destination = vec![0; size];
        let mut destination_io = rimio::MemRimIO::new(&mut destination);
        let io = &mut destination_io;
        {
            let mut formatter = FS::formatter(io, meta);
            formatter.format(false).unwrap();
            formatter.flush().unwrap();
        }
        let mut source = ContractSource {
            fail,
            triggered: core::cell::Cell::new(false),
        };
        {
            let mut injector = FS::injector(io, meta).unwrap();
            let result = injector.inject_tree_from_resolver(&mut source, "/*");
            if fail {
                assert!(
                    source.triggered.get(),
                    "failure must occur during a payload read"
                );
                let error = result.expect_err("source read failure was swallowed");
                let mut cause = error.source();
                let mut propagated = false;
                while let Some(current) = cause {
                    if current
                        == crate::errors::FsError::IO(rimio::RimIOError::Other(
                            "contract source read failure",
                        ))
                    {
                        propagated = true;
                        break;
                    }
                    cause = current.source();
                }
                assert!(propagated, "source error was replaced: {error:?}");
            } else {
                let counts = result.expect("streaming transfer failed");
                assert_eq!(counts.files, 1);
                assert_eq!(counts.dirs, 1);
                assert_eq!(counts.bytes, 8193);
            }
        }
        if !fail {
            let mut resolver = FS::resolver(io, meta);
            assert_dir_entries(&mut resolver, "transfer", &["data.bin"]);
            assert_dirs(&mut resolver, &["transfer"]);
            let expected = boundary_payload(8193);
            assert_eq!(resolver.read_file("transfer/data.bin").unwrap(), expected);
            drop(resolver);
            assert_no_errors(&FS::checker(io, meta).check_all().unwrap());
        }
        // No rollback or clean-checker guarantee is assumed after failed mutation.
    }
}
