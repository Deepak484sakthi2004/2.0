// verify: release ok
// "The Tail at Scale" (Dean and Barroso, 2013), simulated with a fixed seed. Each backend answers in ~1-3 ms,
// except 1% of calls that take 50 ms. A request fans out to N backends and waits for all of them.
// Then hedging: if a backend hasn't answered after 3 ms, send one backup call and take whichever answers first.
use hdrhistogram::Histogram;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// One backend call's latency in microseconds.
    fn backend_us(&mut self) -> u64 {
        if self.next() % 100 == 0 { 50_000 + self.next() % 10_000 } else { 1_000 + self.next() % 2_000 }
    }
}

fn main() {
    let requests = 100_000;
    println!("{:>7} {:>9} {:>9} {:>11} {:>12}   {:>22}", "fan-out", "p50 (ms)", "p99 (ms)", "slow reqs", "1-0.99^N", "hedged p99 / extra calls");
    for n in [1u32, 10, 50, 100] {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let (mut plain, mut hedged) = (
            Histogram::<u64>::new_with_bounds(1, 10_000_000, 3).unwrap(),
            Histogram::<u64>::new_with_bounds(1, 10_000_000, 3).unwrap(),
        );
        let (mut slow, mut extra) = (0u64, 0u64);
        for _ in 0..requests {
            let (mut worst, mut worst_h) = (0u64, 0u64);
            for _ in 0..n {
                let first = rng.backend_us();
                worst = worst.max(first);
                // Hedge: after 3 ms without an answer, send a second call; the answer is whichever returns first.
                let h = if first > 3_000 {
                    extra += 1;
                    first.min(3_000 + rng.backend_us())
                } else {
                    first
                };
                worst_h = worst_h.max(h);
            }
            if worst >= 50_000 {
                slow += 1;
            }
            plain.record(worst).unwrap();
            hedged.record(worst_h).unwrap();
        }
        println!(
            "{n:>7} {:>9.1} {:>9.1} {:>10.1}% {:>11.1}%   {:>9.1} ms / {:>5.2}%",
            plain.value_at_percentile(50.0) as f64 / 1e3,
            plain.value_at_percentile(99.0) as f64 / 1e3,
            100.0 * slow as f64 / requests as f64,
            100.0 * (1.0 - 0.99f64.powi(n as i32)),
            hedged.value_at_percentile(99.0) as f64 / 1e3,
            100.0 * extra as f64 / (requests as f64 * n as f64)
        );
    }
}
