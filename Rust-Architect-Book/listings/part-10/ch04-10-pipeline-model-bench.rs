// verify: release ok
// A Rust-side MODEL of what a JVM stream pipeline costs when the JIT can't specialize it:
// boxed elements (Stream<Long> ~ Vec<Box<u64>>) and an indirect call per stage per element
// (a megamorphic lambda site ~ &dyn Fn). This is not a Java measurement: see the chapter's JMH note.
// Best of 7 runs per variant; one Playground run, noisy.
use std::hint::black_box;
use std::time::Instant;

fn time(label: &str, elems: usize, mut f: impl FnMut() -> u64) {
    let mut best = f64::MAX;
    let mut result = 0;
    for _ in 0..7 {
        let t = Instant::now();
        result = black_box(f());
        best = best.min(t.elapsed().as_secs_f64());
    }
    println!("{label:<44} {:>6.2} ns/elem   (result {result})", best * 1e9 / elems as f64);
}

fn main() {
    const N: usize = 1_000_000;
    let flat: Vec<u64> = (0..N as u64).map(|i| i.wrapping_mul(2_654_435_761) % 1_000).collect();
    let boxed: Vec<Box<u64>> = flat.iter().map(|&x| Box::new(x)).collect();

    // The stages, as trait objects the optimizer can't see through.
    let keep: &dyn Fn(u64) -> bool = black_box(&|x: u64| x % 2 == 0);
    let square: &dyn Fn(u64) -> u64 = black_box(&|x: u64| x * x);

    time("static stages, flat u64       (Rust default)", N, || {
        black_box(&flat).iter().filter(|&&x| x % 2 == 0).map(|&x| x * x).sum()
    });
    time("static stages, boxed elements", N, || {
        black_box(&boxed).iter().filter(|x| ***x % 2 == 0).map(|x| **x * **x).sum()
    });
    time("dyn stages,    flat u64", N, || {
        black_box(&flat).iter().filter(|&&x| keep(x)).map(|&x| square(x)).sum()
    });
    time("dyn stages,    boxed elements", N, || {
        black_box(&boxed).iter().filter(|x| keep(***x)).map(|x| square(**x)).sum()
    });
}
