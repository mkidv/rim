// SPDX-License-Identifier: MIT

//! exFAT filesystem driver implementation.

pub use crate::core::traits::*;

use crate::traits::*;
use rimio::RimIO;

pub struct ExFat;

impl<'a> FsFilesystem<'a> for ExFat {
    type Meta = ExFatMeta;
    type Unit = u32;
    type Handle = ExFatHandle;
    type Formatter = ExFatFormatter<'a, dyn RimIO + 'a>;
    type Injector = ExFatInjector<'a, dyn RimIO + 'a>;
    type Checker = ExFatChecker<'a, dyn RimIO + 'a>;
    type Resolver = ExFatResolver<'a, dyn RimIO + 'a>;

    fn formatter(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Formatter {
        ExFatFormatter::new(io, meta)
    }

    fn injector(
        io: &'a mut (dyn RimIO + 'a),
        meta: &'a Self::Meta,
    ) -> crate::core::FsInjectorResult<Self::Injector> {
        ExFatInjector::new(io, meta)
    }

    fn checker(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Checker {
        ExFatChecker::new(io, meta)
    }

    fn resolver(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Resolver {
        ExFatResolver::new(io, meta)
    }

    fn identifier() -> &'static str {
        "exfat"
    }
}
