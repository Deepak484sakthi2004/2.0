// verify: release ok
// Benchmark the failure path at the failure rate production sees (Part VIII's promise):
// Result propagation vs panic + catch_unwind, at call depth 1, 10, 100, for failure rates 0%, 1%, 10%.
use std::hint::black_box;
use std::panic;
use std::time::Instant;

#[inline(never)]
fn result_chain(depth: u32, fail: bool) -> Result<u64, u32> {
    if depth == 0 {
        return if fail { Err(7) } else { Ok(1) };
    }
    Ok(black_box(result_chain(depth - 1, fail))? + 1)
}

#[inline(never)]
fn panic_chain(depth: u32, fail: bool) -> u64 {
    if depth == 0 {
        if fail {
            panic!("declined");
        }
        return 1;
    }
    black_box(panic_chain(depth - 1, fail)) + 1
}

fn main() {
    panic::set_hook(Box::new(|_| {})); // silence the default "thread panicked" message
    let calls = 20_000u32;
    println!("ns per call (release, best of 5 runs of {calls} calls; one Playground run, noisy)");
    println!("{:>6} {:>6} {:>12} {:>16}", "depth", "fail%", "Result", "panic+catch");
    for depth in [1u32, 10, 100] {
        for fail_pct in [0u32, 1, 10] {
            let fail = |i: u32| i % 100 < fail_pct;
            let mut best_r = f64::MAX;
            let mut best_p = f64::MAX;
            for _ in 0..5 {
                let t = Instant::now();
                for i in 0..calls {
                    black_box(result_chain(black_box(depth), fail(i)).is_ok());
                }
                best_r = best_r.min(t.elapsed().as_nanos() as f64 / calls as f64);

                let t = Instant::now();
                for i in 0..calls {
                    black_box(panic::catch_unwind(|| panic_chain(black_box(depth), fail(i))).is_ok());
                }
                best_p = best_p.min(t.elapsed().as_nanos() as f64 / calls as f64);
            }
            println!("{depth:>6} {fail_pct:>5}% {best_r:>12.1} {best_p:>16.1}");
        }
    }
}
