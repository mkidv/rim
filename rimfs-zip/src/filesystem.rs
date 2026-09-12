// SPDX-License-Identifier: MIT

//! ZIP archive driver implementation.

use crate::checker::ZipChecker;
use crate::formatter::ZipFormatter;
use crate::injector::ZipInjector;
use crate::meta::ZipMeta;
use crate::resolver::ZipResolver;
use crate::types::ZipHandle;
use rimfs_core::FsInjectorResult;
use rimfs_core::filesystem::FsFilesystem;
use rimio::RimIO;

/// ZIP filesystem and archive driver.
pub struct Zip;

impl<'a> FsFilesystem<'a> for Zip {
    type Unit = u64;
    type Meta = ZipMeta;
    type Handle = ZipHandle;
    type Formatter = ZipFormatter<'a, dyn RimIO + 'a>;
    type Injector = ZipInjector<'a, dyn RimIO + 'a>;
    type Checker = ZipChecker<'a, dyn RimIO + 'a>;
    type Resolver = ZipResolver<'a, dyn RimIO + 'a>;

    fn formatter(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Formatter {
        ZipFormatter::new(io, meta)
    }

    fn injector(
        io: &'a mut (dyn RimIO + 'a),
        meta: &'a Self::Meta,
    ) -> FsInjectorResult<Self::Injector> {
        ZipInjector::new(io, meta)
    }

    fn checker(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Checker {
        ZipChecker::new(io, meta)
    }

    fn resolver(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Resolver {
        ZipResolver::new(io, meta)
    }

    fn identifier() -> &'static str {
        "zip"
    }
}
