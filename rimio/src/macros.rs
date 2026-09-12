// SPDX-License-Identifier: MIT

//! Convenience macros for I/O operations and assertions.

/// Automatically implements read functions for primitive types on RimRead
#[macro_export]
macro_rules! RimRead_impl_primitive_r {
    ($($ty:ty),+ $(,)?) => {
        $(
            paste::paste! {
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

/// Automatically implements write functions for primitive types on RimWrite
#[macro_export]
macro_rules! RimWrite_impl_primitive_w {
    ($($ty:ty),+ $(,)?) => {
        $(
            paste::paste! {
                #[inline(always)]
                fn [<write_ $ty _at>](&mut self, offset: u64, value: $ty) -> RimIOResult {
                    let buf = value.to_le_bytes();
                    self.write_at(offset, &buf)
                }
            }
        )+
    };
}

/// Automatically implements read/write functions for primitive types on RimIO
#[macro_export]
macro_rules! RimIO_impl_primitive_rw {
    ($($ty:ty),+ $(,)?) => {
        $crate::RimRead_impl_primitive_r!($($ty),+);
        $crate::RimWrite_impl_primitive_w!($($ty),+);
    };
}
