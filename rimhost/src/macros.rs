// SPDX-License-Identifier: MIT

//! Host command construction helper macros.

#[macro_export]
macro_rules! args {
    ($($arg:expr),* $(,)?) => {
        vec![$($arg.to_string()),*]
    };
}
