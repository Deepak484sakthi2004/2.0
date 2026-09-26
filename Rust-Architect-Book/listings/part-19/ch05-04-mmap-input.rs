// verify: release ok
// logstat's input three ways: a reused buffer with read_until (Project L1), read the whole file, or memory-map it.
// Count lines of a ~44 MB log file; report time (best of 3, one run on a shared machine: noisy), minor page faults per
// run, and resident memory while the data is in use, split into anonymous (heap) and file-backed (page cache) pages.
use std::io::{BufRead, BufReader, Write};
use std::time::Instant;

fn status_kib(field: &str) -> i64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap();
    s.lines().find(|l| l.starts_with(field)).unwrap().split_whitespace().nth(1).unwrap().parse().unwrap()
}

fn minor_faults() -> i64 {
    // SAFETY: getrusage writes into the zeroed struct we pass.
    unsafe {
        let mut u: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut u);
        u.ru_minflt
    }
}

const PATH: &str = "/tmp/access.log";

/// Each method returns (lines, RssAnon KiB, RssFile KiB) sampled while its data is still alive.
fn by_read_until() -> (usize, i64, i64) {
    let mut r = BufReader::with_capacity(64 * 1024, std::fs::File::open(PATH).unwrap());
    let mut buf = Vec::with_capacity(256);
    let mut lines = 0;
    while r.read_until(b'\n', &mut buf).unwrap() > 0 {
        lines += 1;
        buf.clear();
    }
    (lines, status_kib("RssAnon"), status_kib("RssFile"))
}

fn by_read_all() -> (usize, i64, i64) {
    let data = std::fs::read(PATH).unwrap();
    let lines = memchr::memchr_iter(b'\n', &data).count();
    (lines, status_kib("RssAnon"), status_kib("RssFile"))
}

fn by_mmap() -> (usize, i64, i64) {
    let f = std::fs::File::open(PATH).unwrap();
    // SAFETY: nothing truncates or writes /tmp/access.log while it is mapped (see ch05-05 for what happens otherwise).
    let map = unsafe { memmap::Mmap::map(&f).unwrap() };
    let lines = memchr::memchr_iter(b'\n', &map).count();
    (lines, status_kib("RssAnon"), status_kib("RssFile"))
}

fn main() {
    {
        let mut w = std::io::BufWriter::new(std::fs::File::create(PATH).unwrap());
        for i in 0..500_000u32 {
            writeln!(w, "10.0.{}.{} - - [26/Sep/2026:08:{:02}:{:02}] \"GET /v1/payments/{i} HTTP/1.1\" 200 {} 0.{:03}",
                i % 250, i % 200, i / 60 % 60, i % 60, 200 + i % 900, i % 997).unwrap();
        }
    }
    println!("file: {} bytes (in the page cache: just written)", std::fs::metadata(PATH).unwrap().len());
    let methods: [(&str, fn() -> (usize, i64, i64)); 3] =
        [("read_until, reused buffer", by_read_until), ("fs::read + memchr", by_read_all), ("mmap + memchr", by_mmap)];
    let (anon0, file0) = (status_kib("RssAnon"), status_kib("RssFile"));
    for (name, f) in methods {
        let mut best = f64::MAX;
        let (mut lines, mut anon, mut file) = (0, 0, 0);
        let f0 = minor_faults();
        for _ in 0..3 {
            let t = Instant::now();
            (lines, anon, file) = f();
            best = best.min(t.elapsed().as_secs_f64() * 1000.0);
        }
        println!("{name:<26} {lines} lines  best {best:>5.1} ms  faults/run {:>6}  while in use: RssAnon {:>+7} KiB  RssFile {:>+7} KiB",
            (minor_faults() - f0) / 3, anon - anon0, file - file0);
    }
}
