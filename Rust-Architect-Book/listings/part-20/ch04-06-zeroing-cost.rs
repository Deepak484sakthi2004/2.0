// verify: release ok
// Chapter 15.3's promise: what does zeroing a read buffer cost, as a function of its size?
// Each "read" copies `n` bytes of input into a buffer. Three ways to get the buffer:
//   fresh:   vec![0u8; n] per read (calloc; large sizes come from mmap as fresh zero pages)
//   zeroed:  one reused buffer, zeroed before every read (clear + resize, the pattern 15.3 profiled)
//   reused:  one reused buffer, initialized once, overwritten by every read (15.3's PooledBuf)
// ns per KiB, best of 5; release; one Playground run, noisy.
use std::hint::black_box;
use std::time::Instant;

fn per_kib(n: usize, reads: usize, mut f: impl FnMut()) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..5 {
        let t = Instant::now();
        for _ in 0..reads {
            f();
        }
        best = best.min(t.elapsed().as_nanos() as f64 / reads as f64 / (n as f64 / 1024.0));
    }
    best
}

fn main() {
    println!("{:>9} {:>10} {:>10} {:>10}", "size", "fresh", "zeroed", "reused");
    for n in [4usize << 10, 16 << 10, 64 << 10, 256 << 10, 1 << 20, 4 << 20] {
        let input = vec![7u8; n];
        let reads = ((64 << 20) / n).max(8); // ~64 MiB of copying per sample
        let fresh = per_kib(n, reads, || {
            let mut buf = vec![0u8; n];
            buf.copy_from_slice(black_box(&input));
            black_box(&buf);
        });
        let mut z = Vec::with_capacity(n);
        let zeroed = per_kib(n, reads, || {
            z.clear();
            z.resize(n, 0u8);
            z.copy_from_slice(black_box(&input));
            black_box(&z);
        });
        let mut r = vec![0u8; n];
        let reused = per_kib(n, reads, || {
            r.copy_from_slice(black_box(&input));
            black_box(&r);
        });
        let label = if n >= 1 << 20 { format!("{} MiB", n >> 20) } else { format!("{} KiB", n >> 10) };
        println!("{label:>9} {fresh:>10.1} {zeroed:>10.1} {reused:>10.1}");
    }
}
