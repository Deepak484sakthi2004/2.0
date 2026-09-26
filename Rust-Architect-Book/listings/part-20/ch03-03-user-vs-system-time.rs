// verify: release ok
// Where does CPU time go: your code (user) or the kernel (system)? getrusage tells you without perf.
// 200,000 small writes to /dev/null, unbuffered vs through a BufWriter (Project L1's choice).
use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::Instant;

fn usage() -> (f64, f64, i64, i64) {
    // SAFETY: getrusage writes into the zeroed struct we pass; RUSAGE_SELF is a valid `who`.
    let ru = unsafe {
        let mut ru: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut ru);
        ru
    };
    let secs = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
    (secs(ru.ru_utime), secs(ru.ru_stime), ru.ru_nvcsw, ru.ru_nivcsw)
}

fn run(label: &str, w: &mut dyn Write) {
    let line = b"GET /v1/ok 200\n"; // 15 bytes
    let (u0, s0, v0, i0) = usage();
    let t = Instant::now();
    for _ in 0..200_000 {
        w.write_all(line).unwrap();
    }
    w.flush().unwrap();
    let wall = t.elapsed().as_secs_f64() * 1e3;
    let (u1, s1, v1, i1) = usage();
    println!(
        "{label:<22} wall {wall:7.1} ms   user {:6.1} ms   system {:6.1} ms   ctx switches: {} voluntary, {} involuntary",
        (u1 - u0) * 1e3,
        (s1 - s0) * 1e3,
        v1 - v0,
        i1 - i0
    );
}

fn main() {
    let mut raw = File::create("/dev/null").unwrap();
    run("unbuffered (1 syscall per line)", &mut raw);
    let mut buffered = BufWriter::with_capacity(64 * 1024, File::create("/dev/null").unwrap());
    run("BufWriter 64 KiB", &mut buffered);
}
