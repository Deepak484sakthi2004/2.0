// verify: debug build
// Instantiation growth: the same generic helper used with 1 row type(s).
// Inspect with: tools\emit.ps1 <this file> -Target llvm-ir -Mode debug   (count `define` lines)
use std::fmt::Debug;

/// A reporting helper written once and called with every row type in the service.
pub fn top_n<T: Ord + Clone + Debug>(rows: &[T], n: usize) -> Vec<String> {
    let mut sorted = rows.to_vec();
    sorted.sort();
    sorted.dedup();
    sorted.iter().rev().take(n).map(|r| format!("{r:?}")).collect()
}

macro_rules! row_types {
    ($($name:ident),* $(,)?) => {
        $(
            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub struct $name(pub u32, pub String);
        )*
        /// Calls top_n once per row type, forcing one instantiation each.
        pub fn report_all(n: usize) -> usize {
            let mut lines = 0;
            $( lines += top_n(&[$name(3, "c".into()), $name(1, "a".into())], n).len(); )*
            lines
        }
    };
}

row_types!(R0,);
