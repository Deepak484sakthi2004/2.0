// verify: release ok
use std::hint::black_box;
use std::panic;
use std::time::Instant;

#[inline(never)]
fn fail_result(depth: u32) -> Result<u32, u32> {
    if depth == 0 {
        return Err(7);
    }
    let v = black_box(fail_result(black_box(depth - 1)))?;
    Ok(v + 1)
}

#[inline(never)]
fn fail_panic(depth: u32) -> u32 {
    if depth == 0 {
        panic!("7");
    }
    black_box(fail_panic(black_box(depth - 1))) + 1
}

fn per_op(n: u32, mut f: impl FnMut()) -> f64 {
    let t = Instant::now();
    for _ in 0..n {
        f();
    }
    t.elapsed().as_nanos() as f64 / n as f64
}

fn main() {
    panic::set_hook(Box::new(|_| {})); // the hook still runs on every panic; it just prints nothing
    let n = 20_000;
    for depth in [1, 10] {
        let r = per_op(n, || {
            black_box(fail_result(black_box(depth)).is_err());
        });
        let p = per_op(n, || {
            black_box(panic::catch_unwind(|| fail_panic(black_box(depth))).is_err());
        });
        println!("depth {depth:>2}: Err returned {r:>8.1} ns/op   panic + catch_unwind {p:>8.1} ns/op   ratio {:>5.0}x", p / r);
    }
}
