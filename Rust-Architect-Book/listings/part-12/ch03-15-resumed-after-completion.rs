// verify: debug panic resumed after completion
//! Polling a finished async fn future again hits the `Returned` state's guard.
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Waker};

async fn answer() -> u32 {
    42
}

fn main() {
    let mut fut = pin!(answer());
    let mut cx = Context::from_waker(Waker::noop());
    println!("first poll:  {:?}", fut.as_mut().poll(&mut cx));
    println!("second poll: {:?}", fut.as_mut().poll(&mut cx)); // a contract violation: panics
}
