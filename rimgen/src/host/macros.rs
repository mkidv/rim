// SPDX-License-Identifier: MIT

#[macro_export]
macro_rules! args {
    ( $( $x:expr ),* ) => {
        vec![ $( $x.to_string() ),* ]
    };
}

pub trait ToArgs {
    fn to_args(self) -> Vec<String>;
}

impl<I, S> ToArgs for I
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    fn to_args(self) -> Vec<String> {
        self.into_iter().map(|s| s.as_ref().to_string()).collect()
    }
}
