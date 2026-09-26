// verify: release ok
// The memory hierarchy, measured: dependent loads (pointer chasing) through a random cycle, one element per 64-byte
// cache line, for working sets from 16 KiB to 256 MiB. Each load must wait for the previous one, so the time per load
// is the latency of whichever level the working set fits in. One Playground run, noisy.
use std::hint::black_box;
use std::time::Instant;

#[repr(align(64))]
#[derive(Clone, Copy)]
struct Line {
    next: usize,
}

fn cache_sizes() -> String {
    (0..5)
        .filter_map(|i| {
            let base = format!("/sys/devices/system/cpu/cpu0/cache/index{i}");
            let level = std::fs::read_to_string(format!("{base}/level")).ok()?;
            let ty = std::fs::read_to_string(format!("{base}/type")).ok()?;
            let size = std::fs::read_to_string(format!("{base}/size")).ok()?;
            Some(format!("L{} {} {}", level.trim(), ty.trim(), size.trim()))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn chase_ns(bytes: usize, loads: usize) -> f64 {
    let n = bytes / 64;
    // A random cyclic permutation (Sattolo's algorithm), so the chain visits every line exactly once per lap
    // and the hardware prefetcher can't guess the next address.
    let mut order: Vec<usize> = (0..n).collect();
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    for i in (1..n).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let j = (x % i as u64) as usize;
        order.swap(i, j);
    }
    let mut lines = vec![Line { next: 0 }; n];
    for i in 0..n {
        lines[order[i]].next = order[(i + 1) % n];
    }
    let mut p = 0usize;
    for _ in 0..n.min(1 << 20) {
        p = lines[p].next; // warm up: bring the set into whatever level it fits
    }
    let t = Instant::now();
    for _ in 0..loads {
        p = lines[p].next;
    }
    let ns = t.elapsed().as_nanos() as f64 / loads as f64;
    black_box(p);
    ns
}

fn main() {
    println!("caches (cpu0): {}", cache_sizes());
    println!("{:>10} {:>12}", "working set", "ns per load");
    for kib in [16usize, 32, 128, 512, 1024, 4096, 16384, 65536, 262144] {
        let loads = 4_000_000;
        let label = if kib >= 1024 { format!("{} MiB", kib / 1024) } else { format!("{kib} KiB") };
        println!("{label:>10} {:>12.1}", chase_ns(kib * 1024, loads));
    }
}
