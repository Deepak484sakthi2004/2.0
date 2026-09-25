// verify: release+nightly error:NoAlias
// verify: debug+nightly error:fn_abi_of
// Nightly-only: #[rustc_abi(debug)] prints the function ABI rustc computes BEFORE handing the
// function to LLVM: how each argument is passed and which attributes it carries. Compare the two
// runs: NoAlias / ReadOnly appear on the references only when optimizing (release).
#![feature(rustc_attrs)]
#![allow(internal_features, dead_code)]

#[rustc_abi(debug)]
fn add_into(dst: &mut i32, src: &i32) {
    *dst += *src;
}

fn main() {}
