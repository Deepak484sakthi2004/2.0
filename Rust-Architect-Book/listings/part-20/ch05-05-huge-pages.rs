// verify: release ok
// Transparent huge pages (Part IX's promise). Map 256 MiB twice: once with MADV_NOHUGEPAGE, once with MADV_HUGEPAGE.
// Touch every 4 KiB page (count minor faults), then do 4M random 8-byte reads (the TLB has to cover 256 MiB).
use std::hint::black_box;
use std::time::Instant;

const SIZE: usize = 256 << 20;
const HUGE: usize = 2 << 20;

fn minor_faults() -> i64 {
    // SAFETY: getrusage writes into the zeroed struct we pass; RUSAGE_SELF is a valid `who`.
    unsafe {
        let mut ru: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut ru);
        ru.ru_minflt
    }
}

fn anon_huge_kib() -> u64 {
    std::fs::read_to_string("/proc/self/smaps_rollup")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("AnonHugePages:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn run(advice: libc::c_int, label: &str) {
    // SAFETY: an anonymous private mapping with no address hint; checked for MAP_FAILED below. We map 2 MiB extra
    // so we can use a 2 MiB-aligned window inside it, and unmap the whole mapping at the end.
    let base = unsafe {
        libc::mmap(std::ptr::null_mut(), SIZE + HUGE, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_PRIVATE | libc::MAP_ANONYMOUS, -1, 0)
    };
    assert_ne!(base, libc::MAP_FAILED);
    let aligned = ((base as usize + HUGE - 1) & !(HUGE - 1)) as *mut u8;
    // SAFETY: `aligned..aligned+SIZE` lies inside the mapping we just created.
    let rc = unsafe { libc::madvise(aligned.cast(), SIZE, advice) };
    // SAFETY: the window is inside our mapping, readable and writable, and nothing else aliases it.
    let mem = unsafe { std::slice::from_raw_parts_mut(aligned, SIZE) };

    let (f0, t0) = (minor_faults(), Instant::now());
    for i in (0..SIZE).step_by(4096) {
        mem[i] = 1; // first touch of every 4 KiB page
    }
    let touch_ms = t0.elapsed().as_secs_f64() * 1e3;
    let faults = minor_faults() - f0;
    let huge_kib = anon_huge_kib();

    let mut x = 0x2545_F491_4F6C_DD1Du64;
    let t = Instant::now();
    let mut acc = 0u64;
    for _ in 0..4_000_000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        acc += mem[(x as usize) & (SIZE - 8)] as u64;
    }
    let rand_ns = t.elapsed().as_nanos() as f64 / 4e6;
    black_box(acc);
    println!(
        "{label:<16} madvise rc {rc}: first touch {touch_ms:6.1} ms, {faults:>6} minor faults, AnonHugePages {:>6} MiB; random read {rand_ns:5.1} ns",
        huge_kib / 1024
    );
    // SAFETY: unmapping exactly the mapping created above; `mem` is not used afterwards.
    unsafe { libc::munmap(base, SIZE + HUGE) };
}

fn main() {
    for f in ["enabled", "defrag"] {
        let s = std::fs::read_to_string(format!("/sys/kernel/mm/transparent_hugepage/{f}")).unwrap_or_else(|_| "unavailable".into());
        println!("THP {f}: {}", s.trim());
    }
    run(libc::MADV_NOHUGEPAGE, "4 KiB pages");
    run(libc::MADV_HUGEPAGE, "huge pages");
}
