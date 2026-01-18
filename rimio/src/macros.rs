/// Automatically implements read/write functions for primitive types on RimIO
#[macro_export]
macro_rules! RimIO_impl_primitive_rw {
    ($($ty:ty),+ $(,)?) => {
        $(
            paste::paste! {
                #[inline(always)]
                fn [<write_ $ty _at>](&mut self, offset: u64, value: $ty) -> RimIOResult {
                    let buf = value.to_le_bytes();
                    self.write_at(offset, &buf)
                }

                #[inline(always)]
                fn [<read_ $ty _at>](&mut self, offset: u64) -> RimIOResult<$ty> {
                    let mut buf = [0u8; core::mem::size_of::<$ty>()];
                    self.read_at(offset, &mut buf)?;
                    Ok(<$ty>::from_le_bytes(buf))
                }
            }
        )+
    };
}
