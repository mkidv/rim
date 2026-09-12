// SPDX-License-Identifier: MIT

//! ISO 9660 filesystem driver implementation.

use crate::checker::IsoChecker;
use crate::formatter::IsoFormatter;
use crate::injector::IsoInjector;
use crate::meta::IsoMeta;
use crate::resolver::IsoResolver;
use crate::types::IsoHandle;
use rimfs_core::FsInjectorResult;
use rimfs_core::filesystem::FsFilesystem;
use rimio::RimIO;

/// ISO 9660 optical disk and hybrid image driver.
pub struct Iso;

impl<'a> FsFilesystem<'a> for Iso {
    type Unit = u32;
    type Meta = IsoMeta;
    type Handle = IsoHandle;
    type Formatter = IsoFormatter<'a, dyn RimIO + 'a>;
    type Injector = IsoInjector<'a, dyn RimIO + 'a>;
    type Checker = IsoChecker<'a, dyn RimIO + 'a>;
    type Resolver = IsoResolver<'a, dyn RimIO + 'a>;

    fn formatter(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Formatter {
        IsoFormatter::new(io, meta)
    }

    fn injector(
        io: &'a mut (dyn RimIO + 'a),
        meta: &'a Self::Meta,
    ) -> FsInjectorResult<Self::Injector> {
        IsoInjector::new(io, meta)
    }

    fn checker(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Checker {
        IsoChecker::new(io, meta)
    }

    fn resolver(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Resolver {
        IsoResolver::new(io, meta)
    }

    fn identifier() -> &'static str {
        "iso"
    }
}
