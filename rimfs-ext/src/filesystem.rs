// SPDX-License-Identifier: MIT

//! ext2/3/4 filesystem driver implementation.

pub use crate::core::traits::*;

use crate::traits::*;
use rimio::RimIO;

pub struct Ext;

impl<'a> FsFilesystem<'a> for Ext {
    type Meta = ExtMeta;
    type Unit = u32;
    type Handle = ExtHandle;
    type Formatter = ExtFormatter<'a, dyn RimIO + 'a>;
    type Injector = ExtInjector<'a, dyn RimIO + 'a>;
    type Checker = ExtChecker<'a, dyn RimIO + 'a>;
    type Resolver = crate::resolver::ExtResolver<'a, dyn RimIO + 'a>;

    fn formatter(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Formatter {
        ExtFormatter::new(io, meta)
    }

    fn injector(
        io: &'a mut (dyn RimIO + 'a),
        meta: &'a Self::Meta,
    ) -> crate::core::FsInjectorResult<Self::Injector> {
        ExtInjector::new(io, meta)
    }

    fn checker(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Checker {
        ExtChecker::new(io, meta)
    }

    fn resolver(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Resolver {
        crate::resolver::ExtResolver::new(io, meta)
    }

    fn identifier() -> &'static str {
        "ext4"
    }
}
