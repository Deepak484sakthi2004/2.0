// verify: release ok
// verify: debug ok
// Where "zero-cost" holds and where it doesn't. Best of 7 runs per variant; one Playground run, noisy.
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
    println!("{label:<38} {:>6.2} ns/elem   (result {result})", best * 1e9 / elems as f64);
}

fn main() {
    const N: usize = 1_000_000;
    let data: Vec<u64> = (0..N as u64).map(|i| i.wrapping_mul(2_654_435_761) % 1_000).collect();
    let (a, b) = data.split_at(N / 2);
    let nested: Vec<Vec<u64>> = data.chunks(4).map(|c| c.to_vec()).collect(); // 250,000 small Vecs

    let d = black_box(&data);
    time("index loop (even squares)", N, || {
        let mut t = 0;
        for i in 0..d.len() {
            if d[i] % 2 == 0 {
                t += d[i] * d[i];
            }
        }
        t
    });
    time("iterator chain (even squares)", N, || d.iter().filter(|&&x| x % 2 == 0).map(|&x| x * x).sum());
    time("chain(): for loop (next)", N, || {
        let mut t = 0;
        for x in black_box(a).iter().chain(black_box(b).iter()) {
            t += *x;
        }
        t
    });
    time("chain(): sum (fold)", N, || black_box(a).iter().chain(black_box(b).iter()).sum());
    time("Box<dyn Iterator>: sum", N, || {
        // black_box hides the concrete type, so LLVM can't devirtualize the calls
        let it: Box<dyn Iterator<Item = &u64>> = black_box(Box::new(d.iter()));
        it.sum()
    });
    time("Box<dyn Iterator>: sum, type visible", N, || {
        let it: Box<dyn Iterator<Item = &u64>> = Box::new(d.iter()); // LLVM sees which vtable this is
        it.sum()
    });
    time("Vec<Vec>: flatten().sum()", N, || black_box(&nested).iter().flatten().sum());
    time("Vec<Vec>: for over flatten (next)", N, || {
        let mut t = 0;
        for x in black_box(&nested).iter().flatten() {
            t += *x;
        }
        t
    });
    time("Vec<Vec>: nested for loops", N, || {
        let mut t = 0;
        for v in black_box(&nested) {
            for x in v {
                t += *x;
            }
        }
        t
    });
    time("collect() mid-pipeline, then sum", N, || {
        let evens: Vec<u64> = d.iter().copied().filter(|x| x % 2 == 0).collect();
        evens.iter().map(|x| x * x).sum()
    });
}
