// verify: debug+nightly error:rustc_dump_def_parents
// Nightly-only compiler introspection: print the DefIds of an item and all its parents.
// DefId(crate:index) is session-local; incremental compilation keys on stable DefPathHashes instead.
#![feature(rustc_attrs)]
#![allow(internal_features, dead_code)]

mod ledger {
    pub mod accounts {
        pub fn balance() -> i64 {
            #[rustc_dump_def_parents]
            fn helper() -> i64 {
                0
            }
            helper()
        }
    }
}

fn main() {}
