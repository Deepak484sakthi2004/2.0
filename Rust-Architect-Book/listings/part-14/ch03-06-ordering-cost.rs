// verify: release ok
// Single-threaded cost of each ordering on x86-64 (uncontended, cache line already local).
// One run on a shared machine: treat the numbers as rough.
use std::hint::black_box;
use std::sync::atomic::{fence, AtomicU64, Ordering::*};
use std::time::Instant;

const N: u64 = 20_000_000;

fn time(name: &str, mut f: impl FnMut(u64)) {
    for i in 0..N / 10 { f(i) } // warm up
    let t = Instant::now();
    for i in 0..N { f(i) }
    let ns = t.elapsed().as_nanos() as f64 / N as f64;
    println!("{name:<28} {ns:>6.2} ns/op");
}

fn main() {
    let a = AtomicU64::new(0);
    let a = black_box(&a);
    time("store(Relaxed)   mov", |i| a.store(i, Relaxed));
    time("store(Release)   mov", |i| a.store(i, Release));
    time("store(SeqCst)    xchg", |i| a.store(i, SeqCst));
    time("load(Relaxed)    mov", |_| { black_box(a.load(Relaxed)); });
    time("load(SeqCst)     mov", |_| { black_box(a.load(SeqCst)); });
    time("fetch_add(Relaxed) lock xadd", |_| { black_box(a.fetch_add(1, Relaxed)); });
    time("fetch_add(SeqCst)  lock xadd", |_| { black_box(a.fetch_add(1, SeqCst)); });
    time("store(Relaxed) + fence(SeqCst)", |i| { a.store(i, Relaxed); fence(SeqCst) });
    println!("final value: {}", a.load(Relaxed));
}
