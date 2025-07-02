pub use crate::core::traits::*;

use rimio::BlockIO;

use crate::fs::exfat::traits::*;

pub struct ExFat;

impl<'a> FsFilesystem<'a> for ExFat {
    type Meta = ExFatMeta;
    type AllocUnit = u32;
    type Handle = ExFatHandle;
    type Allocator = ExFatAllocator<'a>;
    type Formatter = ExFatFormatter<'a, dyn BlockIO + 'a>;
    type Injector = ExFatInjector<'a, dyn BlockIO + 'a>;
    type Checker = ExFatChecker<'a, dyn BlockIO + 'a>;
    type Parser = ExFatParser<'a, dyn BlockIO + 'a>;

    fn allocator(meta: &'a Self::Meta) -> Self::Allocator {
        ExFatAllocator::new(meta)
    }

    fn formatter(io: &'a mut (dyn BlockIO + 'a), meta: &'a Self::Meta) -> Self::Formatter {
        ExFatFormatter::new(io, meta)
    }

    fn injector(
        io: &'a mut (dyn BlockIO + 'a),
        allocator: &'a mut Self::Allocator,
        meta: &'a Self::Meta,
    ) -> Self::Injector {
        ExFatInjector::new(io, allocator, meta)
    }

    fn checker(io: &'a mut (dyn BlockIO + 'a), meta: &'a Self::Meta) -> Self::Checker {
        ExFatChecker::new(io, meta)
    }

    fn parser(io: &'a mut (dyn BlockIO + 'a), meta: &'a Self::Meta) -> Self::Parser {
        ExFatParser::new(io, meta)
    }

    fn identifier() -> &'static str {
        "exfat"
    }
}
