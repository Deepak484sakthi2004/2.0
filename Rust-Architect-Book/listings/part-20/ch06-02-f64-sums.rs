// verify: release ok
// Chapter 7.2's promise (the fraud feature library): the f64 sum stays sequential unless you choose another order.
// Speed of each version, and how far apart the answers are. 32,768 values (256 KiB: stays in L2); best of 7.
use std::hint::black_box;
use std::time::Instant;

#[inline(never)]
fn sum_u32(v: &[u32]) -> u32 {
    v.iter().fold(0u32, |a, &x| a.wrapping_add(x))
}

#[inline(never)]
fn sum_f64(v: &[f64]) -> f64 {
    v.iter().sum()
}

#[inline(never)]
fn sum_f64_lanes(v: &[f64]) -> f64 {
    let mut acc = [0.0f64; 8];
    let chunks = v.chunks_exact(8);
    let tail = chunks.remainder();
    for c in chunks {
        for i in 0..8 {
            acc[i] += c[i];
        }
    }
    acc.iter().sum::<f64>() + tail.iter().sum::<f64>()
}

fn ns_per_elem(len: usize, mut f: impl FnMut()) -> f64 {
    let reps = 2_000;
    (0..7)
        .map(|_| {
            let t = Instant::now();
            for _ in 0..reps {
                f();
            }
            t.elapsed().as_nanos() as f64 / (reps * len) as f64
        })
        .fold(f64::MAX, f64::min)
}

fn main() {
    let n = 32_768;
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    // Feature-like values: both signs, magnitudes spread over 12 orders (1e-3 .. 1e9), so summation order matters.
    let f: Vec<f64> = (0..n)
        .map(|_| {
            let mag = 10f64.powi((next() % 13) as i32 - 3);
            ((next() >> 11) as f64 / (1u64 << 53) as f64 - 0.5) * mag
        })
        .collect();
    let u: Vec<u32> = (0..n).map(|_| next() as u32).collect();

    println!("ns per element (best of 7):");
    println!("  u32 wrapping sum          {:.3}", ns_per_elem(n, || { black_box(sum_u32(black_box(&u))); }));
    println!("  f64 sum, source order     {:.3}", ns_per_elem(n, || { black_box(sum_f64(black_box(&f))); }));
    println!("  f64 sum, 8 lanes          {:.3}", ns_per_elem(n, || { black_box(sum_f64_lanes(black_box(&f))); }));

    let (a, b) = (sum_f64(&f), sum_f64_lanes(&f));
    let mut sorted = f.clone();
    sorted.sort_by(|p, q| p.abs().total_cmp(&q.abs())); // smallest magnitudes first: a more accurate order
    let c: f64 = sorted.iter().sum();
    println!("source order {a:.6e}\n8 lanes      {b:.6e}\nsorted |x|   {c:.6e}");
    let rel = (a - b).abs() / a.abs().max(f64::MIN_POSITIVE);
    println!("relative difference, source order vs 8 lanes: {rel:.2e}");
    // The fraud library's rule: different orders must agree within a documented tolerance.
    assert!(rel < 1e-9, "orders disagree beyond tolerance");
    println!("within the 1e-9 tolerance: yes");
}
