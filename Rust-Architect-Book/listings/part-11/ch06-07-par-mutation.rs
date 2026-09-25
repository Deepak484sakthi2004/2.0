// verify: debug error:E0596
//! The Java parallel-stream bug (`list.parallelStream().forEach(x -> total[0] += x)`) doesn't compile with
//! rayon: for_each takes an `Fn + Send + Sync` closure, which can't mutate what it captured.
use rayon::prelude::*;

fn main() {
    let amounts: Vec<u64> = (1..=1_000).collect();
    let mut total = 0u64;
    amounts.par_iter().for_each(|x| total += x);
    println!("{total}");
}
