// SPDX-License-Identifier: MIT
//! NTFS filesystem implementation
//!
//! Implements the FsFilesystem trait for NTFS.

use rimio::RimIO;

use crate::allocator::NtfsHandle;
use crate::checker::NtfsChecker;
use crate::core::{FsInjectorResult, traits::FsFilesystem};
use crate::formatter::NtfsFormatter;
use crate::injector::NtfsInjector;
use crate::meta::NtfsMeta;
use crate::resolver::NtfsResolver;

/// NTFS filesystem type
pub struct Ntfs;

impl<'a> FsFilesystem<'a> for Ntfs {
    type Meta = NtfsMeta;
    type Unit = u64; // LCN (Logical Cluster Number)
    type Handle = NtfsHandle;
    type Formatter = NtfsFormatter<'a, dyn RimIO + 'a>;
    type Injector = NtfsInjector<'a, dyn RimIO + 'a>;
    type Checker = NtfsChecker<'a, dyn RimIO + 'a>;
    type Resolver = NtfsResolver<'a, dyn RimIO + 'a>;

    fn formatter(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Formatter {
        NtfsFormatter::new(io, meta)
    }

    fn injector(
        io: &'a mut (dyn RimIO + 'a),
        meta: &'a Self::Meta,
    ) -> FsInjectorResult<Self::Injector> {
        NtfsInjector::new(io, meta)
    }

    fn checker(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Checker {
        NtfsChecker::new(io, meta)
    }

    fn resolver(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Resolver {
        NtfsResolver::new(io, meta)
    }

    fn identifier() -> &'static str {
        "ntfs"
    }
}
