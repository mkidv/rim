// SPDX-License-Identifier: MIT

use crate::checker::TarChecker;
use crate::formatter::TarFormatter;
use crate::injector::TarInjector;
use crate::meta::TarMeta;
use crate::resolver::TarResolver;
use crate::types::TarHandle;
use rimfs_core::FsInjectorResult;
use rimfs_core::filesystem::FsFilesystem;
use rimio::RimIO;

/// TAR filesystem driver.
pub struct Tar;

impl<'a> FsFilesystem<'a> for Tar {
    type Unit = u64;
    type Meta = TarMeta;
    type Handle = TarHandle;
    type Formatter = TarFormatter<'a, dyn RimIO + 'a>;
    type Injector = TarInjector<'a, dyn RimIO + 'a>;
    type Checker = TarChecker<'a, dyn RimIO + 'a>;
    type Resolver = TarResolver<'a, dyn RimIO + 'a>;

    fn formatter(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Formatter {
        TarFormatter::new(io, meta)
    }

    fn injector(
        io: &'a mut (dyn RimIO + 'a),
        meta: &'a Self::Meta,
    ) -> FsInjectorResult<Self::Injector> {
        TarInjector::new(io, meta)
    }

    fn checker(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Checker {
        TarChecker::new(io, meta)
    }

    fn resolver(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Resolver {
        TarResolver::new(io, meta)
    }

    fn identifier() -> &'static str {
        "tar"
    }
}
