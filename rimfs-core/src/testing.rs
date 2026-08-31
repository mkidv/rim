extern crate alloc;

use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use crate::{
    checker::{Severity, VerifyReport},
    resolver::{FileAttributes, FsNode, FsTreeResolver},
};
use rimio::prelude::VecRimIO;

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
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.code == code && finding.sev == Severity::Error),
        "expected error {code}, got {report:?}"
    );
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
