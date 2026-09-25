// verify: release ok
// verify: debug miri-ok
// A pooled read buffer that stays INITIALIZED: zeroed once when the pool creates it, never again.
// No unsafe, no uninitialized memory, and no per-read memset. Compared with clear() + resize() per read.
use std::hint::black_box;
use std::io::Read;
use std::time::Instant;

/// INVARIANT: `buf.len() == buf.capacity() == SIZE` for the whole life of the buffer (all bytes initialized);
/// only `buf[..filled]` holds data from the current read. Bytes after `filled` are stale and never exposed.
struct PooledBuf {
    buf: Vec<u8>,
    filled: usize,
}

impl PooledBuf {
    fn new(size: usize) -> Self {
        PooledBuf { buf: vec![0; size], filled: 0 } // the only zeroing, once per pooled buffer
    }
    fn read_from(&mut self, r: &mut dyn Read) -> std::io::Result<&[u8]> {
        self.filled = r.read(&mut self.buf)?; // `read` gets an initialized &mut [u8]: nothing to zero
        Ok(&self.buf[..self.filled]) // expose only what this read produced
    }
}

fn zero_per_read(buf: &mut Vec<u8>, r: &mut dyn Read, n: usize) -> std::io::Result<usize> {
    buf.clear();
    buf.resize(n, 0); // memset(n) on every read
    r.read(buf)
}

fn main() {
    const SIZE: usize = 16 * 1024;
    let iters = if cfg!(miri) { 3 } else { 50_000 };
    let payload = vec![7u8; SIZE];

    let mut pooled = PooledBuf::new(SIZE);
    let t = Instant::now();
    let mut sum_a = 0u64;
    for _ in 0..iters {
        let mut src: &[u8] = &payload;
        let r: &mut dyn Read = black_box(&mut src as &mut dyn Read); // opaque reader: a real call, like a socket
        sum_a += black_box(pooled.read_from(r).unwrap()).len() as u64;
    }
    let a = t.elapsed();

    let mut v = Vec::with_capacity(SIZE);
    let t = Instant::now();
    let mut sum_b = 0u64;
    for _ in 0..iters {
        let mut src: &[u8] = &payload;
        let r: &mut dyn Read = black_box(&mut src as &mut dyn Read);
        sum_b += zero_per_read(&mut v, r, SIZE).unwrap() as u64;
        black_box(&v);
    }
    let b = t.elapsed();

    assert_eq!(sum_a, sum_b);
    if !cfg!(miri) {
        println!("{iters} reads of {} KiB:", SIZE / 1024);
        println!("  pooled, initialized once : {:>8.0} ns/read", a.as_secs_f64() * 1e9 / iters as f64);
        println!("  clear + resize per read  : {:>8.0} ns/read", b.as_secs_f64() * 1e9 / iters as f64);
    } else {
        println!("miri: {sum_a} bytes read both ways");
    }
}
