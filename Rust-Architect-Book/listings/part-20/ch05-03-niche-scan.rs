// verify: release ok
// Chapter 5.2's Systems exercise: sum a slice of Option<u64> (16 bytes each) vs Option<NonZeroU64> (8 bytes each)
// on 1K, 1M and 16M elements (the exercise said 100M; the Playground's memory limit says 16M). Median of 11 samples.
use std::hint::black_box;
use std::num::NonZeroU64;
use std::time::Instant;

#[inline(never)]
fn tagged_in_memory(v: &[Option<u64>]) -> u64 {
    v.iter().flatten().sum()
}

#[inline(never)]
fn niche_in_memory(v: &[Option<NonZeroU64>]) -> u64 {
    v.iter().flatten().map(|x| x.get()).sum()
}

fn median_ns_per_elem(len: usize, mut f: impl FnMut() -> u64) -> f64 {
    let reps = (16_000_000 / len).clamp(1, 10_000);
    let mut v: Vec<f64> = (0..11)
        .map(|_| {
            let t = Instant::now();
            for _ in 0..reps {
                black_box(f());
            }
            t.elapsed().as_nanos() as f64 / (reps * len) as f64
        })
        .collect();
    v.sort_by(f64::total_cmp);
    v[5]
}

fn main() {
    println!("{:>10} {:>14} {:>14} {:>7}   (ns per element, median of 11)", "elements", "Option<u64>", "Option<NonZero>", "ratio");
    for len in [1_000usize, 1_000_000, 16_000_000] {
        let tagged: Vec<Option<u64>> = (0..len as u64).map(|i| if i % 10 == 0 { None } else { Some(i) }).collect();
        let niche: Vec<Option<NonZeroU64>> = (0..len as u64).map(|i| if i % 10 == 0 { None } else { NonZeroU64::new(i) }).collect();
        assert_eq!(tagged_in_memory(&tagged), niche_in_memory(&niche));
        let t = median_ns_per_elem(len, || tagged_in_memory(black_box(&tagged)));
        let n = median_ns_per_elem(len, || niche_in_memory(black_box(&niche)));
        let mb = |bytes: usize| bytes as f64 / 1e6;
        println!(
            "{len:>10} {t:>14.3} {n:>14.3} {:>7.2}   ({:.0} MB vs {:.0} MB)",
            t / n,
            mb(len * 16),
            mb(len * 8)
        );
    }
}
