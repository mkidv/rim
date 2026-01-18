// SPDX-License-Identifier: MIT

use rimio::RimIO;

use crate::core::traits::*;
use crate::fs::ext4::traits::*;

pub struct Ext4;

impl<'a> FsFilesystem<'a> for Ext4 {
    type Meta = Ext4Meta;
    type AllocUnit = u32;
    type Handle = Ext4Handle;
    type Allocator = Ext4Allocator<'a>;
    type Formatter = Ext4Formatter<'a, dyn RimIO + 'a>;
    type Injector = Ext4Injector<'a, dyn RimIO + 'a>;
    type Checker = Ext4Checker<'a, dyn RimIO + 'a>;
    type Parser = crate::fs::ext4::resolver::Ext4Resolver<'a, dyn RimIO + 'a>;

    fn allocator(meta: &'a Self::Meta) -> Self::Allocator {
        Ext4Allocator::new(meta)
    }

    fn formatter(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Formatter {
        Ext4Formatter::new(io, meta)
    }

    fn injector(
        io: &'a mut (dyn RimIO + 'a),
        allocator: &'a mut Self::Allocator,
        meta: &'a Self::Meta,
    ) -> crate::core::FsInjectorResult<Self::Injector> {
        Ok(Ext4Injector::new(io, allocator, meta))
    }

    fn checker(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Checker {
        Ext4Checker::new(io, meta)
    }

    fn parser(io: &'a mut (dyn RimIO + 'a), meta: &'a Self::Meta) -> Self::Parser {
        crate::fs::ext4::resolver::Ext4Resolver::new(io, meta)
    }

    fn identifier() -> &'static str {
        "ext4"
    }
}
