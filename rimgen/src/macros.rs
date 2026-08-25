// SPDX-License-Identifier: MIT

#[macro_export]
macro_rules! gen_error_wiring {
    (
        top => $top:ty {
            $($top_src:ty : $top_variant:ident),+ $(,)?
        },
        str_into => [ $($str_tgt:ty),* $(,)? ],
        sub => {
            $($src_sub:ty => [ $($dst_sub:ident::$dst_variant:ident),+ ] ),* $(,)?
        } $(,)?
    ) => {
        $crate::__impl_into_generror!{ $top; $( $top_src => $top_variant ),+ }
        $crate::__impl_str_into_generrors!{ $top; $( $str_tgt ),* }
        $crate::__impl_sub_into_generror!{ $( $src_sub => [ $( $dst_sub :: $dst_variant ),+ ] ),* }
    };
}

#[macro_export]
macro_rules! __impl_into_generror {
    ($top:ty; $($t:ty => $variant:ident),+ $(,)?) => {
        $(
            impl From<$t> for $top {
                #[inline]
                fn from(e: $t) -> Self { <$top>::$variant(e) }
            }
        )+
    }
}

#[macro_export]
macro_rules! __impl_str_into_generrors {
    ($top:ty; $($t:ty),* $(,)?) => {
        $(
            impl From<&'static str> for $t {
                #[inline]
                fn from(msg: &'static str) -> Self { <$t>::Other(msg) }
            }
        )*
        impl From<&'static str> for $top {
            #[inline]
            fn from(msg: &'static str) -> Self { <$top>::Other(msg) }
        }
    }
}

#[macro_export]
macro_rules! __impl_sub_into_generror {
    ($($src:ty => [ $( $dst:ident::$variant:ident ),+ ] ),* $(,)?) => {
        $(
            $(
                impl From<$src> for $dst {
                    #[inline]
                    fn from(e: $src) -> Self { <$dst>::$variant(e.into()) }
                }
            )+
        )*
    }
}

#[macro_export]
macro_rules! ensure {
    ($cond:expr, $err:expr) => {
        if !$cond {
            return Err($err.into());
        }
    };
}

#[macro_export]
macro_rules! bail {
    ($err:expr) => {
        return Err($err.into());
    };
}
