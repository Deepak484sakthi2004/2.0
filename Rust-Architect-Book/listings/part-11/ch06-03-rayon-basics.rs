// verify: debug ok
//! Rayon: data parallelism with work stealing. Same answers as the sequential code, by construction.
use rayon::prelude::*;

/// Divide and conquer with rayon::join: "these two may run in parallel if a thread is free".
fn sum(xs: &[u64]) -> u64 {
    if xs.len() <= 4_096 {
        return xs.iter().sum();
    }
    let (left, right) = xs.split_at(xs.len() / 2);
    let (a, b) = rayon::join(|| sum(left), || sum(right));
    a + b
}

fn main() {
    println!("rayon global pool: {} threads", rayon::current_num_threads());

    let amounts: Vec<u64> = (1..=1_000_000).collect();
    let seq: u64 = amounts.iter().map(|x| x % 97).sum();
    let par: u64 = amounts.par_iter().map(|x| x % 97).sum();
    println!("par_iter sum == sequential: {} ({par})", par == seq);
    println!("rayon::join sum: {}", sum(&amounts));

    let mut prices: Vec<u32> = (0..200_000u32).map(|i| i.wrapping_mul(2_654_435_761) % 100_000).collect();
    prices.par_chunks_mut(10_000).for_each(|chunk| chunk.sort_unstable()); // sort each chunk in parallel
    let sorted_chunks = prices.chunks(10_000).all(|c| c.is_sorted());
    prices.par_sort_unstable(); // parallel sort of the whole thing
    println!("chunks sorted: {sorted_chunks}; whole vector sorted: {}", prices.is_sorted());

    let over_limit = amounts.par_iter().filter(|&&x| x % 1_000 == 0).count();
    let first_big = amounts.par_iter().find_first(|&&x| x > 999_990);
    println!("filter/count: {over_limit}; find_first: {first_big:?}");
}
