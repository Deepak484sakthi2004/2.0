// verify: debug error:E0658
//! std has an MPMC channel, but on Rust 1.98 it is still unstable (library feature `mpmc_channel`).
//! On stable, multi-consumer means crossbeam-channel (listing ch05-04).
use std::sync::mpmc;

fn main() {
    let (tx, rx) = mpmc::channel::<u32>();
    let rx2 = rx.clone(); // two consumers of one channel
    tx.send(1).unwrap();
    println!("{:?} {:?}", rx.try_recv(), rx2.try_recv());
}
