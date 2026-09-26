// verify: release ok
// The first pass over fresh memory pays for page faults; the second doesn't. Same code, same data size.
use std::hint::black_box;
use std::time::Instant;

fn minor_faults() -> i64 {
    // SAFETY: getrusage writes into the zeroed struct we pass; RUSAGE_SELF is a valid `who`.
    unsafe {
        let mut ru: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut ru);
        ru.ru_minflt
    }
}

fn write_pass(buf: &mut [u8]) {
    for chunk in buf.chunks_mut(4096) {
        chunk[0] = chunk[0].wrapping_add(1); // touch one byte per 4 KiB page
    }
}

fn main() {
    let size = 128 << 20; // 128 MiB
    let mut buf = vec![0u8; size]; // calloc: the kernel maps zero pages lazily
    for pass in 1..=3 {
        let f0 = minor_faults();
        let t = Instant::now();
        write_pass(black_box(&mut buf));
        let ms = t.elapsed().as_secs_f64() * 1e3;
        println!("pass {pass}: {ms:7.2} ms, {:>6} minor page faults", minor_faults() - f0);
    }
    println!("pages in the buffer: {}", size / 4096);
    black_box(&buf);
}
