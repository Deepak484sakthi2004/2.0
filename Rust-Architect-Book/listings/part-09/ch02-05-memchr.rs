// verify: release ok
// One run on a shared machine: noisy. Compare ratios.
use std::hint::black_box;
use std::time::Instant;

fn main() {
    // ~16 MiB of log-like ASCII: timestamps, paths, an email on 1 line in 50.
    let mut log = String::with_capacity(17 << 20);
    let mut i = 0u64;
    while log.len() < 16 << 20 {
        log.push_str(&format!("2026-09-25T10:{:02}:{:02}Z GET /api/orders/{} 200 {}us", i % 60, (i / 60) % 60, i * 7919 % 100_000, i % 997));
        if i % 50 == 0 {
            log.push_str(" user=ops@example.com");
        }
        log.push('\n');
        i += 1;
    }
    let hay = log.as_bytes();
    println!("haystack: {} MiB, {} lines", hay.len() >> 20, i);

    // 1. Find every '@' (rare byte).
    let t = Instant::now();
    let (mut pos, mut n1) = (0, 0);
    while let Some(off) = black_box(&hay[pos..]).iter().position(|&b| b == b'@') {
        n1 += 1;
        pos += off + 1;
    }
    let t_naive = t.elapsed();
    let t = Instant::now();
    let n2 = memchr::memchr_iter(b'@', black_box(hay)).count();
    let t_memchr = t.elapsed();
    assert_eq!(n1, n2);
    println!("rare byte '@' ({n1} hits):   iter().position loop {t_naive:>9.2?} | memchr_iter {t_memchr:>9.2?}");

    // 2. Count every '\n' (common byte): a plain filter().count() also vectorizes.
    let t = Instant::now();
    let c1 = black_box(hay).iter().filter(|&&b| b == b'\n').count();
    let t_filter = t.elapsed();
    let t = Instant::now();
    let c2 = memchr::memchr_iter(b'\n', black_box(hay)).count();
    let t_memchr = t.elapsed();
    assert_eq!(c1, c2);
    println!("common byte '\\n' ({c1} hits): filter().count()    {t_filter:>9.2?} | memchr_iter {t_memchr:>9.2?}");

    // 3. A substring.
    let t = Instant::now();
    let s1 = black_box(log.as_str()).matches("user=").count();
    let t_std = t.elapsed();
    let t = Instant::now();
    let s2 = memchr::memmem::find_iter(black_box(hay), b"user=").count();
    let t_memmem = t.elapsed();
    assert_eq!(s1, s2);
    println!("substring \"user=\" ({s1} hits): str::matches        {t_std:>9.2?} | memmem      {t_memmem:>9.2?}");

    // 4. A candidate CLASS of 13 bytes (digits, '@', 'B', 'p'): too many for memchr3. Use a 256-entry table.
    let mut class = [false; 256];
    for b in b"0123456789@Bp" {
        class[*b as usize] = true;
    }
    let t = Instant::now();
    let k1 = black_box(hay).iter().filter(|&&b| class[b as usize]).count();
    let t_table = t.elapsed();
    let t = Instant::now();
    let k2 = memchr::memchr3_iter(b'@', b'B', b'p', black_box(hay)).count();
    let t_m3 = t.elapsed();
    println!("class of 13 bytes: table scan {k1} candidates {t_table:>9.2?} | memchr3 on '@','B','p' only: {k2} candidates {t_m3:>9.2?}");
    println!("digits alone are {:.0}% of all bytes", 100.0 * (k1 - k2) as f64 / hay.len() as f64);
}
