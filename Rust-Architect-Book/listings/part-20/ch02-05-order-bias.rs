// verify: release ok
// Two IDENTICAL functions, benchmarked naively (one timed call each, A first) and then properly.
use std::hint::black_box;
use std::time::Instant;

#[inline(never)]
fn variant_a(v: &[u64]) -> u64 {
    v.iter().fold(0, |acc, &x| acc ^ x.rotate_left(7))
}

#[inline(never)]
fn variant_b(v: &[u64]) -> u64 {
    v.iter().fold(0, |acc, &x| acc ^ x.rotate_left(7))
}

fn once(f: fn(&[u64]) -> u64, v: &[u64]) -> f64 {
    let t = Instant::now();
    black_box(f(black_box(v)));
    t.elapsed().as_secs_f64() * 1e3
}

fn main() {
    // A freshly allocated, zeroed 64 MiB buffer: the kernel maps its pages lazily.
    let v = vec![0u64; 8 << 20];

    // The naive benchmark: time each variant once, A first.
    let a = once(variant_a, &v);
    let b = once(variant_b, &v);
    println!("naive, A then B:  A {a:6.2} ms   B {b:6.2} ms   -> \"B is {:.0}x faster\"", a / b);

    // The same naive benchmark with the order reversed, on a new buffer.
    let v2 = vec![0u64; 8 << 20];
    let b2 = once(variant_b, &v2);
    let a2 = once(variant_a, &v2);
    println!("naive, B then A:  A {a2:6.2} ms   B {b2:6.2} ms   -> \"A is {:.0}x faster\"", b2 / a2);

    // The harness: warm-up, then 15 interleaved samples of each, compare medians.
    for _ in 0..3 {
        black_box(variant_a(&v));
        black_box(variant_b(&v));
    }
    let (mut sa, mut sb) = (Vec::new(), Vec::new());
    for _ in 0..15 {
        sa.push(once(variant_a, &v));
        sb.push(once(variant_b, &v));
    }
    sa.sort_by(f64::total_cmp);
    sb.sort_by(f64::total_cmp);
    println!(
        "warm, interleaved: A median {:.2} ms [{:.2}..{:.2}]   B median {:.2} ms [{:.2}..{:.2}]",
        sa[7], sa[0], sa[14], sb[7], sb[0], sb[14]
    );
    drop(v2);
}
