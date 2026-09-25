// verify: release ok
//! Cloning and dropping an Arc is an atomic increment and decrement of a shared counter.
//! When many threads do it to the SAME Arc, that counter's cache line bounces between cores. One run, noisy.
use std::rc::Rc;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

const PER_THREAD: u32 = 2_000_000;

fn per_op(f: impl FnOnce(), ops: u32) -> f64 {
    let t = Instant::now();
    f();
    t.elapsed().as_nanos() as f64 / ops as f64
}

fn main() {
    let threads = thread::available_parallelism().unwrap().get() as u32;

    let rc = Rc::new(0u64);
    let rc_ns = per_op(
        || {
            for _ in 0..PER_THREAD {
                std::hint::black_box(Rc::clone(std::hint::black_box(&rc)));
            }
        },
        PER_THREAD,
    );

    let arc = Arc::new(0u64);
    let arc_ns = per_op(
        || {
            for _ in 0..PER_THREAD {
                std::hint::black_box(Arc::clone(std::hint::black_box(&arc)));
            }
        },
        PER_THREAD,
    );

    // Every thread clones the same Arc: one contended counter.
    let shared = Arc::new(0u64);
    let shared_ns = per_op(
        || {
            thread::scope(|s| {
                for _ in 0..threads {
                    s.spawn(|| {
                        for _ in 0..PER_THREAD {
                            std::hint::black_box(Arc::clone(std::hint::black_box(&shared)));
                        }
                    });
                }
            })
        },
        PER_THREAD, // wall time per op per thread
    );

    // Every thread clones its own Arc: no sharing.
    let own_ns = per_op(
        || {
            thread::scope(|s| {
                for _ in 0..threads {
                    s.spawn(|| {
                        let mine = Arc::new(0u64);
                        for _ in 0..PER_THREAD {
                            std::hint::black_box(Arc::clone(std::hint::black_box(&mine)));
                        }
                    });
                }
            })
        },
        PER_THREAD,
    );

    println!("clone + drop, 1 thread,  Rc:                    {rc_ns:>6.1} ns");
    println!("clone + drop, 1 thread,  Arc:                   {arc_ns:>6.1} ns");
    println!("clone + drop, {threads} threads, one shared Arc:      {shared_ns:>6.1} ns (wall time per op per thread)");
    println!("clone + drop, {threads} threads, one Arc per thread:  {own_ns:>6.1} ns (wall time per op per thread)");
}
