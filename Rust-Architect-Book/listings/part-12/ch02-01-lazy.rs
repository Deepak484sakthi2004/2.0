// verify: debug ok
//! Calling an async fn runs none of its body. It builds a value (a future); the body runs when
//! someone polls it. Here we poll by hand, with a waker that does nothing (Waker::noop, 1.85).
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Waker};

async fn charge(amount: u64) -> u64 {
    println!("   (charge body runs: {amount})");
    amount
}

fn main() {
    println!("1. calling charge(500)");
    let fut = charge(500); // no body runs: this only stores `amount` in a state machine
    println!("2. got a {}-byte future; the body hasn't run", std::mem::size_of_val(&fut));

    let mut fut = pin!(fut);
    let mut cx = Context::from_waker(Waker::noop());
    println!("3. first poll");
    let result = fut.as_mut().poll(&mut cx);
    println!("4. poll returned {result:?}");

    let never_polled = charge(700);
    drop(never_polled);
    println!("5. dropped a second future without polling it: its body never ran");
}
