// verify: release ok
// The book's benchmark harness (a small stand-in for criterion, which isn't on the Playground):
// warm-up, many samples, candidates interleaved within each round, medians with a spread, and a verdict
// that refuses to call a difference smaller than the noise.
use std::hint::black_box;
use std::time::Instant;

struct Stats {
    min: f64,
    p10: f64,
    median: f64,
    p90: f64,
    max: f64,
}

fn stats(mut v: Vec<f64>) -> Stats {
    v.sort_by(f64::total_cmp);
    let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    Stats { min: v[0], p10: at(0.10), median: at(0.50), p90: at(0.90), max: v[v.len() - 1] }
}

/// ns per call for each candidate: `warmup` untimed rounds, then `samples` rounds; in each round every candidate
/// runs `iters` calls, in rotating order, so drift (frequency, noisy neighbours) hits all candidates alike.
fn compare(cands: &mut [(&str, &mut dyn FnMut())], warmup: u32, samples: usize, iters: u32) -> Vec<Stats> {
    for _ in 0..warmup {
        for (_, f) in cands.iter_mut() {
            f();
        }
    }
    let n = cands.len();
    let mut per: Vec<Vec<f64>> = vec![Vec::with_capacity(samples); n];
    for s in 0..samples {
        for k in 0..n {
            let i = (s + k) % n; // rotate who goes first
            let f = &mut cands[i].1;
            let t = Instant::now();
            for _ in 0..iters {
                f();
            }
            per[i].push(t.elapsed().as_nanos() as f64 / iters as f64);
        }
    }
    per.into_iter().map(stats).collect()
}

fn verdict(a: (&str, &Stats), b: (&str, &Stats)) -> String {
    if a.1.p90 < b.1.p10 {
        format!("{} is faster: {:.2}x at the median", a.0, b.1.median / a.1.median)
    } else if b.1.p90 < a.1.p10 {
        format!("{} is faster: {:.2}x at the median", b.0, a.1.median / b.1.median)
    } else {
        "no difference detectable at this noise level (the p10..p90 ranges overlap)".to_string()
    }
}

fn report(title: &str, names: &[&str], st: &[Stats]) {
    println!("{title}");
    for (name, s) in names.iter().zip(st) {
        println!(
            "  {name:<24} median {:>9.1} ns   p10..p90 [{:.1} .. {:.1}]   min {:.1}  max {:.1}",
            s.median, s.p10, s.p90, s.min, s.max
        );
    }
}

fn main() {
    // 1. Two ways to write the same sum: expect "no difference".
    let v: Vec<u64> = (0..100_000).collect();
    let mut iter_sum = || {
        black_box(black_box(&v).iter().sum::<u64>());
    };
    let mut index_sum = || {
        let v = black_box(&v);
        let mut s = 0u64;
        for i in 0..v.len() {
            s += v[i];
        }
        black_box(s);
    };
    let names = ["iter().sum()", "index loop"];
    let st = compare(&mut [(names[0], &mut iter_sum as &mut dyn FnMut()), (names[1], &mut index_sum)], 3, 31, 20);
    report("sum of 100,000 u64 (ns per call)", &names, &st);
    println!("  verdict: {}", verdict((names[0], &st[0]), (names[1], &st[1])));

    // 2. Stable vs unstable sort of the same 10,000 pseudo-random u32 (each call sorts a fresh copy).
    let mut x = 0x9E37_79B9u32;
    let data: Vec<u32> = (0..10_000)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        })
        .collect();
    let mut buf_a = data.clone();
    let mut buf_b = data.clone();
    let mut stable = || {
        buf_a.copy_from_slice(black_box(&data));
        buf_a.sort();
        black_box(&buf_a);
    };
    let mut unstable = || {
        buf_b.copy_from_slice(black_box(&data));
        buf_b.sort_unstable();
        black_box(&buf_b);
    };
    let names = ["sort (stable)", "sort_unstable"];
    let st = compare(&mut [(names[0], &mut stable as &mut dyn FnMut()), (names[1], &mut unstable)], 3, 31, 5);
    report("sort 10,000 u32 (ns per call, copy included in both)", &names, &st);
    println!("  verdict: {}", verdict((names[0], &st[0]), (names[1], &st[1])));
}
