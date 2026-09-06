// SPDX-License-Identifier: MIT

use std::collections::HashMap;
use std::time::Instant;

use rimfs_core::allocator::FsHandle;
use rimfs_core::errors::{FsInjectorError, FsResolverError};
use rimfs_core::injector::FsTreeInjector;
use rimfs_core::resolver::FsTreeResolver;
use rimfs_core::resolver::attr::{FileAttributes, NodeKind};
use rimfs_core::utils::path_utils::{
    extract_name_from_path, is_wildcard, join_paths, strip_wildcard,
};
use rimio::prelude::{RimIOResult, RimRead};

use super::error::{CopyError, CopyResult};
use super::options::{CopyOptions, MetadataPolicy, OverwritePolicy, UnsupportedMetadataPolicy};
use super::progress::CopyEvent;
use super::report::{CopyReport, CopyWarning, CopyWarningKind};

/// Streaming adapter that intercepts `read_at` calls to trigger progress updates and track bytes read.
struct ProgressReader<'a, R: RimRead + ?Sized, F: FnMut(u64, u64)> {
    inner: &'a mut R,
    total_size: u64,
    bytes_read: u64,
    on_chunk: F,
}

impl<'a, R: RimRead + ?Sized, F: FnMut(u64, u64)> RimRead for ProgressReader<'a, R, F> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
        self.inner.read_at(offset, buf)?;
        self.bytes_read += buf.len() as u64;
        let current_pos = offset.saturating_add(buf.len() as u64).min(self.total_size);
        (self.on_chunk)(current_pos, self.total_size);
        Ok(())
    }

    fn total_size(&mut self) -> RimIOResult<u64> {
        Ok(self.total_size)
    }
}

/// Simple reader that tracks total bytes read without progress events.
struct CountingReader<'a, R: RimRead + ?Sized> {
    inner: &'a mut R,
    bytes_read: u64,
}

impl<'a, R: RimRead + ?Sized> RimRead for CountingReader<'a, R> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
        self.inner.read_at(offset, buf)?;
        self.bytes_read += buf.len() as u64;
        Ok(())
    }

    fn total_size(&mut self) -> RimIOResult<u64> {
        self.inner.total_size()
    }
}

/// Applies the metadata preservation policy to file attributes.
fn apply_metadata_policy(attr: &FileAttributes, policy: MetadataPolicy) -> FileAttributes {
    match policy {
        MetadataPolicy::PreserveAll => attr.clone(),
        MetadataPolicy::PreserveBasic => {
            let mut a = attr.clone();
            a.mode = None;
            a.uid = None;
            a.gid = None;
            a
        }
        MetadataPolicy::Strip => match attr.kind {
            NodeKind::Directory => FileAttributes::new_dir(),
            NodeKind::Symlink => FileAttributes::new_symlink(),
            _ => FileAttributes::new_file(),
        },
    }
}

/// Transfers a logical filesystem tree or entry from any [FsTreeResolver] to any compatible [FsTreeInjector].
///
/// Streaming data transfers happen directly between the resolver file reader and the injector file writer,
/// with no intermediate file buffering.
pub fn copy_tree<H: FsHandle, R: FsTreeResolver + ?Sized, I: FsTreeInjector<H> + ?Sized>(
    resolver: &mut R,
    injector: &mut I,
    source_path: &str,
    options: &CopyOptions,
    mut progress: Option<&mut dyn FnMut(CopyEvent<'_>)>,
) -> CopyResult<CopyReport> {
    let start_time = Instant::now();
    let mut report = CopyReport::default();

    if options.overwrite_policy == OverwritePolicy::Replace && !options.destination_supports_replace
    {
        return Err(CopyError::UnsupportedFeature {
            path: source_path.to_string(),
            details: "In-place entry replacement ('--overwrite replace') is unsupported on filesystem images; supported only on Host destinations".to_string(),
        });
    }

    let is_wild = is_wildcard(source_path);
    let base_path = if is_wild {
        strip_wildcard(source_path)
    } else {
        source_path
    };

    let root_attr = if is_wild || base_path.is_empty() || base_path == "/" {
        FileAttributes::new_dir()
    } else {
        resolver
            .read_attributes(base_path)
            .map_err(|e| CopyError::Resolver {
                path: base_path.to_string(),
                source: e,
            })?
    };

    let adjusted_root_attr = apply_metadata_policy(&root_attr, options.metadata_policy);
    injector
        .set_root_context(&adjusted_root_attr)
        .map_err(|e| CopyError::Injector {
            path: base_path.to_string(),
            source: e,
        })?;

    if is_wild || base_path.is_empty() || base_path == "/" {
        // Copy directory contents directly into root context
        let lookup_path = if base_path.is_empty() { "/" } else { base_path };
        let entries = resolver
            .read_dir(lookup_path)
            .map_err(|e| CopyError::Resolver {
                path: lookup_path.to_string(),
                source: e,
            })?;

        let mut seen_entries: HashMap<String, String> = HashMap::new();

        for entry_name in entries {
            if !options.destination_case_sensitive && options.detect_case_collisions {
                let lower = entry_name.to_lowercase();
                if let Some(existing) = seen_entries.get(&lower) {
                    match options.overwrite_policy {
                        OverwritePolicy::Error => {
                            return Err(CopyError::CaseCollision {
                                directory: lookup_path.to_string(),
                                entry: entry_name.clone(),
                                existing: existing.clone(),
                            });
                        }
                        OverwritePolicy::Skip => {
                            let warning = CopyWarning::new(
                                CopyWarningKind::CaseCollision {
                                    directory: lookup_path.to_string(),
                                    entry: entry_name.clone(),
                                    existing: existing.clone(),
                                },
                                format!(
                                    "Case collision in '{lookup_path}': '{entry_name}' conflicts with '{existing}'; skipping to prevent data loss"
                                ),
                            );
                            if let Some(p) = progress.as_deref_mut() {
                                p(CopyEvent::Warning { warning: &warning });
                            }
                            report.warnings.push(warning);
                            report.files_skipped += 1;
                            continue;
                        }
                        OverwritePolicy::Replace => {
                            let warning = CopyWarning::new(
                                CopyWarningKind::CaseCollision {
                                    directory: lookup_path.to_string(),
                                    entry: entry_name.clone(),
                                    existing: existing.clone(),
                                },
                                format!(
                                    "Case collision in '{lookup_path}': '{entry_name}' conflicts with '{existing}'; replacing"
                                ),
                            );
                            if let Some(p) = progress.as_deref_mut() {
                                p(CopyEvent::Warning { warning: &warning });
                            }
                            report.warnings.push(warning);
                        }
                    }
                } else {
                    seen_entries.insert(lower, entry_name.clone());
                }
            }

            let child_path = join_paths(lookup_path, &entry_name);
            copy_entry_recursive(
                resolver,
                injector,
                &child_path,
                &entry_name,
                options,
                &mut report,
                &mut progress,
            )?;
        }
    } else if root_attr.is_dir() {
        let entry_name = extract_name_from_path(base_path);
        copy_entry_recursive(
            resolver,
            injector,
            base_path,
            entry_name,
            options,
            &mut report,
            &mut progress,
        )?;
    } else {
        let entry_name = extract_name_from_path(base_path);
        copy_file_entry(
            resolver,
            injector,
            base_path,
            entry_name,
            &root_attr,
            options,
            &mut report,
            &mut progress,
        )?;
    }

    injector.flush().map_err(|e| CopyError::Injector {
        path: "<root>".to_string(),
        source: e,
    })?;

    report.duration = start_time.elapsed();
    Ok(report)
}

fn copy_entry_recursive<H: FsHandle, R: FsTreeResolver + ?Sized, I: FsTreeInjector<H> + ?Sized>(
    resolver: &mut R,
    injector: &mut I,
    path: &str,
    name: &str,
    options: &CopyOptions,
    report: &mut CopyReport,
    progress: &mut Option<&mut dyn FnMut(CopyEvent<'_>)>,
) -> CopyResult<()> {
    let attr = resolver
        .read_attributes(path)
        .map_err(|e| CopyError::Resolver {
            path: path.to_string(),
            source: e,
        })?;

    match attr.kind {
        NodeKind::Directory => {
            if let Some(p) = progress {
                p(CopyEvent::StartingDirectory { path });
            }

            let dir_attr = apply_metadata_policy(&attr, options.metadata_policy);
            injector
                .write_dir(name, &dir_attr)
                .map_err(|e| CopyError::Injector {
                    path: path.to_string(),
                    source: e,
                })?;
            report.directories_created += 1;

            let entries = resolver.read_dir(path).map_err(|e| CopyError::Resolver {
                path: path.to_string(),
                source: e,
            })?;

            let mut seen_entries: HashMap<String, String> = HashMap::new();

            for child_name in entries {
                if !options.destination_case_sensitive && options.detect_case_collisions {
                    let lower = child_name.to_lowercase();
                    if let Some(existing) = seen_entries.get(&lower) {
                        match options.overwrite_policy {
                            OverwritePolicy::Error => {
                                return Err(CopyError::CaseCollision {
                                    directory: path.to_string(),
                                    entry: child_name.clone(),
                                    existing: existing.clone(),
                                });
                            }
                            OverwritePolicy::Skip => {
                                let warning = CopyWarning::new(
                                    CopyWarningKind::CaseCollision {
                                        directory: path.to_string(),
                                        entry: child_name.clone(),
                                        existing: existing.clone(),
                                    },
                                    format!(
                                        "Case collision in '{path}': '{child_name}' conflicts with '{existing}'; skipping to prevent data loss"
                                    ),
                                );
                                if let Some(p) = progress {
                                    p(CopyEvent::Warning { warning: &warning });
                                }
                                report.warnings.push(warning);
                                report.files_skipped += 1;
                                continue;
                            }
                            OverwritePolicy::Replace => {
                                let warning = CopyWarning::new(
                                    CopyWarningKind::CaseCollision {
                                        directory: path.to_string(),
                                        entry: child_name.clone(),
                                        existing: existing.clone(),
                                    },
                                    format!(
                                        "Case collision in '{path}': '{child_name}' conflicts with '{existing}'; replacing"
                                    ),
                                );
                                if let Some(p) = progress {
                                    p(CopyEvent::Warning { warning: &warning });
                                }
                                report.warnings.push(warning);
                            }
                        }
                    } else {
                        seen_entries.insert(lower, child_name.clone());
                    }
                }

                let child_path = join_paths(path, &child_name);
                copy_entry_recursive(
                    resolver,
                    injector,
                    &child_path,
                    &child_name,
                    options,
                    report,
                    progress,
                )?;
            }

            injector.flush_current().map_err(|e| CopyError::Injector {
                path: path.to_string(),
                source: e,
            })?;
        }
        NodeKind::Regular => {
            copy_file_entry(
                resolver, injector, path, name, &attr, options, report, progress,
            )?;
        }
        NodeKind::Symlink => {
            let target = resolver.read_link(path).map_err(|e| CopyError::Resolver {
                path: path.to_string(),
                source: e,
            })?;

            let link_attr = apply_metadata_policy(&attr, options.metadata_policy);
            match injector.write_symlink(name, &target, &link_attr) {
                Ok(()) => {
                    report.symlinks_created += 1;
                    if let Some(p) = progress {
                        p(CopyEvent::CreatedSymlink {
                            path,
                            target: &target,
                        });
                    }
                }
                Err(FsInjectorError::Unsupported(reason)) => match options.unsupported_policy {
                    UnsupportedMetadataPolicy::Error => {
                        return Err(CopyError::UnsupportedFeature {
                            path: path.to_string(),
                            details: format!("Symlink creation unsupported: {reason}"),
                        });
                    }
                    UnsupportedMetadataPolicy::Warn => {
                        let warning = CopyWarning::new(
                            CopyWarningKind::UnsupportedSymlink {
                                path: path.to_string(),
                                target: target.clone(),
                                reason: reason.to_string(),
                            },
                            format!("Skipped symlink '{path}' -> '{target}': {reason}"),
                        );
                        if let Some(p) = progress {
                            p(CopyEvent::Warning { warning: &warning });
                        }
                        report.warnings.push(warning);
                    }
                    UnsupportedMetadataPolicy::Ignore => {}
                },
                Err(e) => {
                    return Err(CopyError::Injector {
                        path: path.to_string(),
                        source: e,
                    });
                }
            }
        }
        _ => {
            return Err(CopyError::UnsupportedFeature {
                path: path.to_string(),
                details: "Unknown or unsupported entry kind".to_string(),
            });
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn copy_file_entry<H: FsHandle, R: FsTreeResolver + ?Sized, I: FsTreeInjector<H> + ?Sized>(
    resolver: &mut R,
    injector: &mut I,
    path: &str,
    name: &str,
    attr: &FileAttributes,
    options: &CopyOptions,
    report: &mut CopyReport,
    progress: &mut Option<&mut dyn FnMut(CopyEvent<'_>)>,
) -> CopyResult<()> {
    let mut reader = resolver.open_file(path).map_err(|e| CopyError::Resolver {
        path: path.to_string(),
        source: e,
    })?;

    let size = reader.total_size().map_err(|e| CopyError::Resolver {
        path: path.to_string(),
        source: FsResolverError::IO(e),
    })?;

    if let Some(p) = progress {
        p(CopyEvent::StartingFile { path, size });
    }

    let file_attr = apply_metadata_policy(attr, options.metadata_policy);

    let bytes_read = if let Some(p) = progress {
        let mut progress_reader = ProgressReader {
            inner: reader.as_mut(),
            total_size: size,
            bytes_read: 0,
            on_chunk: |written, total| {
                p(CopyEvent::FileProgress {
                    path,
                    bytes_written: written,
                    total_bytes: total,
                });
            },
        };
        injector
            .write_file(name, &mut progress_reader, size, &file_attr)
            .map_err(|e| CopyError::Injector {
                path: path.to_string(),
                source: e,
            })?;
        progress_reader.bytes_read
    } else {
        let mut counting_reader = CountingReader {
            inner: reader.as_mut(),
            bytes_read: 0,
        };
        injector
            .write_file(name, &mut counting_reader, size, &file_attr)
            .map_err(|e| CopyError::Injector {
                path: path.to_string(),
                source: e,
            })?;
        counting_reader.bytes_read
    };

    if size > 0 && bytes_read == 0 {
        // The injector skipped writing this file without reading bytes
        report.files_skipped += 1;
        let warning = CopyWarning::new(
            CopyWarningKind::SkippedFile {
                path: path.to_string(),
                reason: "Destination file already exists and overwrite policy is skip".to_string(),
            },
            format!("Skipped '{path}': already exists at destination"),
        );
        if let Some(p) = progress {
            p(CopyEvent::Warning { warning: &warning });
        }
        report.warnings.push(warning);
    } else {
        if let Some(p) = progress {
            p(CopyEvent::FinishedFile { path, size });
        }
        report.files_copied += 1;
        report.bytes_transferred += size;
    }

    Ok(())
}
