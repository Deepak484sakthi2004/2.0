// verify: release ok
// How a sampling profiler works, rebuilt in-process: the worker keeps a tiny "stack" of phase names in atomics,
// a sampler thread reads it periodically, and the samples are printed as folded stacks, the input format of
// flamegraph.pl / inferno. Two views: on-CPU samples only (what `perf record` shows) and wall-clock samples
// (what an off-CPU / wall-clock profiler shows, including time spent waiting). Run twice: a fixed sampling
// interval, and a randomly jittered one.
use std::collections::BTreeMap;
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering::*, fence};
use std::thread;
use std::time::{Duration, Instant};

const NAMES: [&str; 6] = ["idle", "handle", "parse", "auth", "wait_upstream", "serialize"];
static SEQ: AtomicU64 = AtomicU64::new(0); // odd while the worker changes the stack: a seqlock (Chapter 14.4)
static DEPTH: AtomicU8 = AtomicU8::new(0);
static STACK: [AtomicU8; 4] = [const { AtomicU8::new(0) }; 4];
static OFF_CPU: AtomicBool = AtomicBool::new(false);
static DONE: AtomicBool = AtomicBool::new(false);

fn write_begin() {
    SEQ.fetch_add(1, Relaxed);
    fence(Release);
}
fn write_end() {
    SEQ.fetch_add(1, Release);
}

struct Phase;
impl Phase {
    fn enter(id: u8) -> Phase {
        write_begin();
        let d = DEPTH.load(Relaxed);
        STACK[d as usize].store(id, Relaxed);
        DEPTH.store(d + 1, Relaxed);
        write_end();
        Phase
    }
}
impl Drop for Phase {
    fn drop(&mut self) {
        write_begin();
        DEPTH.fetch_sub(1, Relaxed);
        write_end();
    }
}

fn spin(rounds: u64) -> u64 {
    (0..rounds).fold(1u64, |h, i| (h ^ i).wrapping_mul(0x100_0000_01B3))
}

fn handle_request() {
    let _h = Phase::enter(1);
    {
        let _p = Phase::enter(2);
        black_box(spin(black_box(20_000)));
    }
    {
        let _p = Phase::enter(3);
        black_box(spin(black_box(50_000)));
    }
    {
        let _p = Phase::enter(4);
        OFF_CPU.store(true, Relaxed);
        thread::sleep(Duration::from_micros(300)); // blocked on the "upstream": no CPU used
        OFF_CPU.store(false, Relaxed);
    }
    {
        let _p = Phase::enter(5);
        black_box(spin(black_box(30_000)));
    }
}

/// One consistent snapshot of the worker's stack, or None if the worker was mid-update.
fn read_stack() -> Option<(String, bool)> {
    let s0 = SEQ.load(Acquire);
    if s0 % 2 == 1 {
        return None;
    }
    let d = (DEPTH.load(Relaxed) as usize).min(4);
    let ids: Vec<u8> = (0..d).map(|i| STACK[i].load(Relaxed)).collect();
    let off = OFF_CPU.load(Relaxed);
    fence(Acquire);
    if SEQ.load(Relaxed) != s0 {
        return None;
    }
    let frames: Vec<&str> = ids.iter().map(|&i| NAMES[i as usize]).collect();
    let folded = if frames.is_empty() { "idle".to_string() } else { format!("main;{}", frames.join(";")) };
    Some((folded, off))
}

fn profile(jitter: bool) {
    DONE.store(false, Relaxed);
    let sampler = thread::spawn(move || {
        let (mut on_cpu, mut wall) = (BTreeMap::<String, u32>::new(), BTreeMap::<String, u32>::new());
        let mut rng = 0x2545_F491_4F6C_DD1Du64;
        while !DONE.load(Relaxed) {
            if let Some((stack, off)) = read_stack() {
                *wall.entry(stack.clone()).or_default() += 1;
                if !off {
                    *on_cpu.entry(stack).or_default() += 1;
                }
            }
            let us = if jitter {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                50 + rng % 301 // uniform in 50..=350 us, mean 200 us
            } else {
                200
            };
            thread::sleep(Duration::from_micros(us));
        }
        (on_cpu, wall)
    });

    let t = Instant::now();
    let mut requests = 0;
    while t.elapsed() < Duration::from_millis(1200) {
        handle_request();
        requests += 1;
    }
    DONE.store(true, Relaxed);
    let (on_cpu, wall) = sampler.join().unwrap();

    println!("== sampling interval: {} ==", if jitter { "random 50..350 us" } else { "fixed 200 us" });
    for (title, map) in [("on-CPU samples", &on_cpu), ("wall-clock samples", &wall)] {
        let total: u32 = map.values().sum();
        println!("{title}: {total} samples over {requests} requests");
        for (stack, n) in map {
            println!("  {stack} {n}    ({:.0}%)", 100.0 * *n as f64 / total as f64);
        }
    }
}

fn main() {
    profile(false);
    profile(true);
    // Ground truth for the CPU phases, measured directly (spin rounds 20k / 50k / 30k).
    let cost = |r: u64| {
        let t = Instant::now();
        for _ in 0..200 {
            black_box(spin(black_box(r)));
        }
        t.elapsed().as_secs_f64() * 1e6 / 200.0
    };
    let (p, a, s) = (cost(20_000), cost(50_000), cost(30_000));
    let sum = p + a + s;
    println!(
        "direct timing: parse {p:.0} us ({:.0}%), auth {a:.0} us ({:.0}%), serialize {s:.0} us ({:.0}%) of on-CPU time",
        100.0 * p / sum,
        100.0 * a / sum,
        100.0 * s / sum
    );
}
