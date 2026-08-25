// SPDX-License-Identifier: MIT

#[macro_export]
macro_rules! args {
    ($($arg:expr),* $(,)?) => {
        vec![$($arg.to_string()),*]
    };
}
