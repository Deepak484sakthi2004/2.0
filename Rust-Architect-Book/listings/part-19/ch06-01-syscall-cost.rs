// verify: release ok
// The ladder from Rust to the kernel, measured: getpid through std, libc, libc's generic syscall(), and a raw
// `syscall` instruction; then clock_gettime through the vDSO (no kernel entry) vs forced through a real system call.
// Timings: best of 5 runs of 200,000 calls, release, one run on a shared machine: noisy.
use std::time::Instant;

#[inline(never)]
fn raw_getpid() -> i64 {
    let ret: i64;
    // SAFETY: getpid (syscall 39 on x86-64 Linux) takes no arguments, can't fail, and touches no memory.
    // The `syscall` instruction clobbers rcx and r11.
    unsafe {
        std::arch::asm!("syscall", inlateout("rax") 39i64 => ret, lateout("rcx") _, lateout("r11") _, options(nostack));
    }
    ret
}

fn per_call_ns(f: impl Fn() -> i64) -> f64 {
    const N: u32 = 200_000;
    let mut best = f64::MAX;
    for _ in 0..5 {
        let t = Instant::now();
        let mut acc = 0i64;
        for _ in 0..N {
            acc = acc.wrapping_add(std::hint::black_box(f()));
        }
        std::hint::black_box(acc);
        best = best.min(t.elapsed().as_nanos() as f64 / f64::from(N));
    }
    best
}

fn main() {
    // SAFETY (all closures below): getpid/syscall(SYS_getpid) have no preconditions; clock_gettime writes into `ts`.
    let pids = [std::process::id() as i64, unsafe { libc::getpid() } as i64, unsafe { libc::syscall(libc::SYS_getpid) }, raw_getpid()];
    println!("the same pid four ways: {pids:?}");
    let rows: [(&str, Box<dyn Fn() -> i64>); 7] = [
        ("std::process::id()", Box::new(|| std::process::id() as i64)),
        ("libc::getpid()", Box::new(|| unsafe { libc::getpid() } as i64)),
        ("libc::syscall(SYS_getpid)", Box::new(|| unsafe { libc::syscall(libc::SYS_getpid) })),
        ("asm! syscall (rax = 39)", Box::new(raw_getpid)),
        ("Instant::now()", Box::new(|| Instant::now().elapsed().as_nanos() as i64)),
        ("clock_gettime (vDSO)", Box::new(|| {
            let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
            unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
            ts.tv_nsec
        })),
        ("syscall(SYS_clock_gettime)", Box::new(|| {
            let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
            unsafe { libc::syscall(libc::SYS_clock_gettime, libc::CLOCK_MONOTONIC, &mut ts) };
            ts.tv_nsec
        })),
    ];
    for (name, f) in rows.iter() {
        println!("{name:<28} {:>7.1} ns/call", per_call_ns(f));
    }
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
    println!("vdso mapping: {}", maps.lines().find(|l| l.ends_with("[vdso]")).unwrap());
}
