#[macro_export]
macro_rules! fs_error_wiring {
    (
        top => $top:ty {
            $($top_src:ty : $top_variant:ident),+ $(,)?   // sub-errors -> FsError::<Variant>
        },
        str_into => [ $($str_tgt:ty),* $(,)? ],           // &str -> each tgt::Other + top::Other
        sub => {
            $($src_sub:ty => [ $($dst_sub:ident::$dst_variant:ident),+ ] ),* $(,)?  // S -> D::Variant
        } $(,)?
    ) => {
        // Sub-errors -> FsError::<Variant>
        $crate::__impl_into_fserror!{ $top; $( $top_src => $top_variant ),+ }

        // &str -> each::Other + top::Other
        $crate::__impl_str_into_errors!{ $top; $( $str_tgt ),* }

        // Inter-layer conversions
        $crate::__impl_sub_into_error!{ $( $src_sub => [ $( $dst_sub :: $dst_variant ),+ ] ),* }
    };
}

#[macro_export]
macro_rules! __impl_into_fserror {
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
macro_rules! __impl_str_into_errors {
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
macro_rules! __impl_sub_into_error {
    ($($src:ty => [ $( $dst:ident::$variant:ident ),+ ] ),* $(,)?) => {
        $(
            $(
                impl From<$src> for $dst {
                    #[inline]
                    fn from(e: $src) -> Self { <$dst>::$variant(e) }
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
