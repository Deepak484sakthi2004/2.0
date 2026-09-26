// verify: release ok
// Allocator behaviour under threads (Part IX's promise). Each "request" allocates 32 small buffers (16..512 bytes),
// writes them, and frees them. Three setups, on 1, 2 and 4 threads:
//   local:   each thread allocates and frees its own memory with the system allocator (glibc malloc)
//   handoff: thread A allocates, sends the buffers over a channel, thread B frees them (cross-thread frees)
//   arena:   each thread bump-allocates from its own bumpalo::Bump and resets it after every request
// One Playground run, noisy.
use bumpalo::Bump;
use std::hint::black_box;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

const REQUESTS: usize = 40_000;
const PER_REQ: usize = 32;

fn size(i: usize, r: usize) -> usize {
    16 << ((i + r) % 6) // 16, 32, 64, 128, 256, 512
}

fn local(r0: usize) {
    let mut bufs: Vec<Vec<u8>> = Vec::with_capacity(PER_REQ);
    for r in r0..r0 + REQUESTS {
        for i in 0..PER_REQ {
            let mut b = Vec::with_capacity(size(i, r));
            b.push(i as u8);
            bufs.push(b);
        }
        black_box(&bufs);
        bufs.clear(); // frees all 32 on this thread
    }
}

fn arena(r0: usize) {
    let mut bump = Bump::with_capacity(32 * 1024);
    for r in r0..r0 + REQUESTS {
        let mut total = 0usize;
        for i in 0..PER_REQ {
            let b = bump.alloc_slice_fill_copy(size(i, r), 0u8);
            b[0] = i as u8;
            total += b.len();
        }
        black_box(total);
        bump.reset(); // "frees" everything at once: one pointer reset
    }
}

/// ns per request per thread, for `threads` threads each running `f`.
fn per_thread(threads: usize, f: fn(usize)) -> f64 {
    let t = Instant::now();
    let hs: Vec<_> = (0..threads).map(|k| thread::spawn(move || f(k * 7))).collect();
    for h in hs {
        h.join().unwrap();
    }
    t.elapsed().as_nanos() as f64 / REQUESTS as f64
}

/// `pairs` producer/consumer pairs. With `cross_free`, the consumer frees what the producer allocated; without it
/// (the control), the producer frees its own buffers and sends only a token, so the channel cost is the same.
fn handoff(pairs: usize, cross_free: bool) -> f64 {
    let t = Instant::now();
    let mut hs = Vec::new();
    for k in 0..pairs {
        let (tx, rx) = mpsc::sync_channel::<Option<Vec<Vec<u8>>>>(64);
        hs.push(thread::spawn(move || {
            for r in 0..REQUESTS {
                let mut bufs = Vec::with_capacity(PER_REQ);
                for i in 0..PER_REQ {
                    let mut b = Vec::with_capacity(size(i, r + k));
                    b.push(i as u8);
                    bufs.push(b);
                }
                if cross_free {
                    tx.send(Some(bufs)).unwrap();
                } else {
                    black_box(&bufs);
                    drop(bufs); // freed on the producer thread
                    tx.send(None).unwrap();
                }
            }
        }));
        hs.push(thread::spawn(move || {
            for msg in rx {
                black_box(&msg);
                drop(msg); // with cross_free: freed on the consumer thread
            }
        }));
    }
    for h in hs {
        h.join().unwrap();
    }
    t.elapsed().as_nanos() as f64 / REQUESTS as f64
}

fn main() {
    println!("available parallelism: {}", thread::available_parallelism().map_or(0, |n| n.get()));
    println!("ns per request (32 alloc+free) per thread, best of 3; release; one Playground run, noisy");
    let best = |f: &dyn Fn() -> f64| (0..3).map(|_| f()).fold(f64::MAX, f64::min);
    for threads in [1usize, 2, 4] {
        let l = best(&|| per_thread(threads, local));
        let a = best(&|| per_thread(threads, arena));
        println!("  {threads} thread(s): malloc local {l:7.0}   bump arena {a:6.0}");
    }
    for pairs in [1usize, 2] {
        let control = best(&|| handoff(pairs, false));
        let cross = best(&|| handoff(pairs, true));
        println!(
            "  {pairs} producer/consumer pair(s): token only (frees local) {control:7.0}   buffers handed off (cross-thread frees) {cross:7.0}"
        );
    }
}
