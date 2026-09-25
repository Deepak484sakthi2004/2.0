// verify: debug ok
// Stream.sorted()/distinct() and JDK 24 Gatherers (windowFixed, windowSliding, scan), translated.
use itertools::Itertools;

fn main() {
    let latencies_ms = [12_u32, 7, 12, 30, 7, 55, 9, 30];

    // sorted() + distinct(): in Rust the materialization is explicit (a Vec you can see).
    let mut v = latencies_ms.to_vec();
    v.sort_unstable();
    v.dedup();
    println!("sorted distinct (Vec):       {v:?}");
    // itertools hides the same buffering behind adapter syntax:
    println!("sorted().unique() (itertools): {:?}", latencies_ms.iter().sorted().unique().collect::<Vec<_>>());

    // Gatherers.windowFixed(3)  -> chunks(3) on a slice
    println!("windowFixed(3):   {:?}", latencies_ms.chunks(3).collect::<Vec<_>>());
    // Gatherers.windowSliding(3) -> windows(3) on a slice (borrowed views, no copies)
    let max_of_3: Vec<u32> = latencies_ms.windows(3).map(|w| *w.iter().max().unwrap()).collect();
    println!("sliding max of 3: {max_of_3:?}");
    // Gatherers.scan(...)        -> scan: running state, one output per input
    let running: Vec<u32> = latencies_ms.iter().scan(0, |acc, &x| { *acc += x; Some(*acc) }).collect();
    println!("running total:    {running:?}");
    // takeWhile on a running condition -> map_while / take_while
    let under_budget: Vec<u32> = running.iter().copied().take_while(|&t| t <= 100).collect();
    println!("within 100 ms:    {under_budget:?}");
    // On any iterator (not just slices), itertools gives windows of tuples:
    let deltas: Vec<i64> = latencies_ms.iter().tuple_windows().map(|(a, b)| *b as i64 - *a as i64).collect();
    println!("deltas:           {deltas:?}");
}
