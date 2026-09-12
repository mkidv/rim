// SPDX-License-Identifier: MIT

//! FAT filesystem driver implementation.

pub use crate::core::traits::*;

use crate::traits::*;
use rimio::RimIO;

pub struct Fat;

impl<'a> FsFilesystem<'a> for Fat {
    type Meta = FatMeta;
    type Unit = u32;
    type Handle = FatHandle;
    type Formatter = FatFormatter<'a, dyn RimIO + 'a>;
    type Injector = FatInjector<'a, dyn RimIO + 'a>;
    type Checker = FatChecker<'a, dyn RimIO + 'a>;
    type Resolver = FatResolver<'a, dyn RimIO + 'a>;

    fn formatter(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Formatter {
        FatFormatter::new(io, meta)
    }

    fn injector(
        io: &'a mut (dyn RimIO + 'a),
        meta: &'a Self::Meta,
    ) -> crate::core::FsInjectorResult<Self::Injector> {
        FatInjector::new(io, meta)
    }

    fn checker(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Checker {
        FatChecker::new(io, meta)
    }

    fn resolver(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Resolver {
        FatResolver::new(io, meta)
    }

    fn identifier() -> &'static str {
        "fat"
    }
}
