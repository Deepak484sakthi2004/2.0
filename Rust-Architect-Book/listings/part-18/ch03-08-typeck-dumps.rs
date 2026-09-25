// verify: debug+nightly error:rustc_dump_item_bounds
// Nightly-only: what type checking decided. (1) The hidden type behind an `impl Trait` return,
// (2) the implicit `Sized` bound on an associated type, (3) the closure's captures.
#![feature(rustc_attrs, stmt_expr_attributes)]
#![allow(internal_features, dead_code)]
#![rustc_dump_hidden_type_of_opaques]

fn evens() -> impl Iterator<Item = u32> {
    (0..10).filter(|x| x % 2 == 0)
}

trait Store {
    #[rustc_dump_item_bounds]
    type Key: Clone + Send;
}

fn main() {
    let route = String::from("/pay");
    let log = #[rustc_capture_analysis]
    || println!("{}", route.len());
    log();
    let _ = evens();
}
