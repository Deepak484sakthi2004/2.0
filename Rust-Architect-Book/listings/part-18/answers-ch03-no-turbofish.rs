// verify: debug+nightly error:EvaluatedToAmbig
// Answer-key check for Chapter 18.3's beginner exercise: without the turbofish, `T` is still an
// inference variable when the callee's where-clauses are first evaluated, so the verdict is ambiguous.
#![feature(rustc_attrs)]
#![allow(internal_features)]

#[rustc_evaluate_where_clauses]
fn needs_clone<T: Clone>(t: T) -> T {
    t.clone()
}

fn main() {
    needs_clone(String::new());
}
