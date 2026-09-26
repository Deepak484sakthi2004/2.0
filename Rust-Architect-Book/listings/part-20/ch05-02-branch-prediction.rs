// verify: release ok
// Branch prediction, measured. 4M bytes; for each byte >= 128, call a small non-inlined function (so the compiler
// must keep a real branch). Same data in three orders: random, sorted, and a repeating pattern. Then the same count
// written so the compiler can drop the branch. One Playground run, noisy.
use std::hint::black_box;
use std::time::Instant;

#[inline(never)]
fn on_big(acc: u64, x: u8) -> u64 {
    acc.wrapping_mul(31).wrapping_add(x as u64)
}

#[inline(never)]
fn branchy(v: &[u8]) -> u64 {
    let mut acc = 0u64;
    for &x in v {
        if x >= 128 {
            acc = on_big(acc, x); // taken ~50% of the time on random data
        }
    }
    acc
}

#[inline(never)]
fn count_big(v: &[u8]) -> usize {
    v.iter().filter(|&&x| x >= 128).count() // no call in the body: LLVM is free to make it branch-free
}

fn ns_per_elem(v: &[u8], f: fn(&[u8]) -> u64) -> f64 {
    (0..5)
        .map(|_| {
            let t = Instant::now();
            black_box(f(black_box(v)));
            t.elapsed().as_nanos() as f64 / v.len() as f64
        })
        .fold(f64::MAX, f64::min)
}

fn main() {
    let n = 4 << 20;
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    let random: Vec<u8> = (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect();
    let mut sorted = random.clone();
    sorted.sort_unstable();
    let pattern: Vec<u8> = (0..n).map(|i| if i % 4 < 2 { 200 } else { 10 }).collect(); // big, big, small, small, ...

    println!("ns per element (best of 5):");
    println!("  branch + call, random order     {:5.2}", ns_per_elem(&random, branchy));
    println!("  branch + call, sorted           {:5.2}", ns_per_elem(&sorted, branchy));
    println!("  branch + call, repeating 2-on-2-off {:5.2}", ns_per_elem(&pattern, branchy));
    let c = |v: &[u8]| count_big(v) as u64;
    println!("  count (branch-free), random     {:5.2}", ns_per_elem(&random, c));
    println!("  count (branch-free), sorted     {:5.2}", ns_per_elem(&sorted, c));
    println!("taken fraction, random: {:.3}", count_big(&random) as f64 / n as f64);
}
