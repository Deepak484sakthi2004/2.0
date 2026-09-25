// verify: debug+nightly error:EvaluatedToAmbig
// Nightly-only: ask the trait solver to report how it evaluates a callee's where-clauses at each
// call site. With a turbofish the answer is known (EvaluatedToOk); with an argument whose type is
// still an inference variable at that moment, it's ambiguous, and the real error (E0277) comes
// later, from the fulfillment pass that re-checks pending obligations as types become known.
#![feature(rustc_attrs)]
#![allow(internal_features, dead_code)]

#[rustc_evaluate_where_clauses]
fn needs_clone<T: Clone>(t: T) -> T {
    t.clone()
}

struct NotClone;

fn main() {
    needs_clone::<String>(String::new());
    needs_clone(NotClone);
}
