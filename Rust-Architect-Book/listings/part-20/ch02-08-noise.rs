// verify: release ok
// The same workload, measured 31 times in a row on a shared machine. How much does it move, and why?
use std::hint::black_box;
use std::time::Instant;

/// (steal, total) CPU ticks from the aggregate "cpu" line of /proc/stat.
fn cpu_ticks() -> (u64, u64) {
    let stat = std::fs::read_to_string("/proc/stat").unwrap_or_default();
    let fields: Vec<u64> = stat
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .skip(1)
        .filter_map(|x| x.parse().ok())
        .collect();
    // user nice system idle iowait irq softirq steal ...
    (fields.get(7).copied().unwrap_or(0), fields.iter().take(8).sum())
}

fn work(v: &[u32]) -> u32 {
    v.iter().fold(0u32, |a, &x| a.wrapping_mul(31).wrapping_add(x)) // a dependent chain: CPU-bound
}

fn main() {
    let v: Vec<u32> = (0..2_000_000).collect();
    let (steal0, total0) = cpu_ticks();
    let mut ms: Vec<f64> = (0..31)
        .map(|_| {
            let t = Instant::now();
            black_box(work(black_box(&v)));
            t.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    let (steal1, total1) = cpu_ticks();
    let first = ms[0];
    ms.sort_by(f64::total_cmp);
    println!("31 runs of the same 2M-element dependent loop (ms):");
    println!("  first run {first:.3}   min {:.3}   median {:.3}   max {:.3}", ms[0], ms[15], ms[30]);
    println!("  spread (max - min) / median = {:.0}%", 100.0 * (ms[30] - ms[0]) / ms[15]);
    println!(
        "  /proc/stat during the runs: {} ticks total, {} ticks of steal (time the hypervisor gave our vCPUs to someone else)",
        total1 - total0,
        steal1 - steal0
    );
}
