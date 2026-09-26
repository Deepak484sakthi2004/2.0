// verify: debug ok
// Demand paging, measured: map memory, touch it, give it back. Resident memory (VmRSS) and minor page faults
// (getrusage) before and after each step; then transparent huge pages; then a thread stack's reservation vs use.
use std::ptr;

fn rss_kib() -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap();
    s.lines().find(|l| l.starts_with("VmRSS")).unwrap().split_whitespace().nth(1).unwrap().parse().unwrap()
}

fn vmsize_kib() -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap();
    s.lines().find(|l| l.starts_with("VmSize")).unwrap().split_whitespace().nth(1).unwrap().parse().unwrap()
}

fn minor_faults() -> i64 {
    // SAFETY: getrusage writes into the zeroed struct we pass.
    unsafe {
        let mut u: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut u);
        u.ru_minflt
    }
}

fn anon_huge_kib() -> u64 {
    let s = std::fs::read_to_string("/proc/self/smaps_rollup").unwrap_or_default();
    s.lines().find(|l| l.starts_with("AnonHugePages")).map_or(0, |l| l.split_whitespace().nth(1).unwrap().parse().unwrap())
}

struct Probe {
    rss: u64,
    faults: i64,
}

impl Probe {
    fn now() -> Probe {
        Probe { rss: rss_kib(), faults: minor_faults() }
    }
    fn report(&mut self, step: &str) {
        let (rss, faults) = (rss_kib(), minor_faults());
        println!("{step:<44} RSS {:>+8} KiB   minor faults {:>+7}", rss as i64 - self.rss as i64, faults - self.faults);
        (self.rss, self.faults) = (rss, faults);
    }
}

/// A function with a 1 MiB stack frame. Entering it touches every page of the frame (stack probes, Part IX interlude).
#[inline(never)]
fn use_stack() -> u8 {
    let mut big = [0u8; 1 << 20];
    for i in (0..big.len()).step_by(4096) {
        big[i] = 7;
    }
    std::hint::black_box(&big)[0]
}

const LEN: usize = 64 << 20; // 64 MiB
const PAGE: usize = 4096;

fn main() {
    let mut p = Probe::now();
    // SAFETY: an anonymous private mapping with no address hint; we check for MAP_FAILED.
    let base = unsafe { libc::mmap(ptr::null_mut(), LEN, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_PRIVATE | libc::MAP_ANONYMOUS, -1, 0) };
    assert_ne!(base, libc::MAP_FAILED);
    let bytes = base as *mut u8;
    p.report("mmap 64 MiB");
    for off in (0..LEN).step_by(PAGE) {
        // SAFETY: off < LEN, inside the mapping we own.
        unsafe { bytes.add(off).write(1) };
    }
    p.report("write 1 byte in each 4 KiB page");
    for off in (0..LEN).step_by(PAGE) {
        // SAFETY: as above.
        unsafe { bytes.add(off).write(2) };
    }
    p.report("write each page again");
    // SAFETY: base/LEN describe our mapping; MADV_DONTNEED drops the pages (anonymous memory reads back as zero).
    unsafe { libc::madvise(base, LEN, libc::MADV_DONTNEED) };
    p.report("madvise(MADV_DONTNEED)");
    // SAFETY: inside the mapping.
    let after = unsafe { bytes.read() };
    p.report(&format!("read page 0 again (value {after})"));
    // SAFETY: base/LEN describe our mapping.
    unsafe { libc::munmap(base, LEN) };
    p.report("munmap");

    // Transparent huge pages: this kernel's mode is "madvise", so ask for them on a 2 MiB-aligned range.
    const HUGE: usize = 2 << 20;
    // SAFETY: as above; we over-allocate by 2 MiB so we can align the start.
    let raw = unsafe { libc::mmap(ptr::null_mut(), LEN + HUGE, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_PRIVATE | libc::MAP_ANONYMOUS, -1, 0) };
    assert_ne!(raw, libc::MAP_FAILED);
    let aligned = ((raw as usize + HUGE - 1) & !(HUGE - 1)) as *mut u8;
    // SAFETY: [aligned, aligned + LEN) lies inside the mapping.
    let rc = unsafe { libc::madvise(aligned as *mut libc::c_void, LEN, libc::MADV_HUGEPAGE) };
    p.report(&format!("mmap 66 MiB + madvise(MADV_HUGEPAGE) = {rc}"));
    for off in (0..LEN).step_by(PAGE) {
        // SAFETY: inside the aligned range.
        unsafe { aligned.add(off).write(1) };
    }
    p.report("write 1 byte in each 4 KiB page");
    let defrag = std::fs::read_to_string("/sys/kernel/mm/transparent_hugepage/defrag").unwrap_or_default();
    println!("{:<44} AnonHugePages {} KiB   (defrag: {})", "", anon_huge_kib(), defrag.trim());
    // SAFETY: raw/LEN + HUGE describe the mapping.
    unsafe { libc::munmap(raw, LEN + HUGE) };
    p.report("munmap");

    // A thread stack: 2 MiB reserved (plus a guard page), a few KiB resident until touched.
    let v0 = vmsize_kib();
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let (tx2, rx2) = std::sync::mpsc::channel::<()>();
    let t = std::thread::spawn(move || {
        tx.send(()).unwrap();
        rx2.recv().unwrap();
        let first = use_stack(); // a 1 MiB frame, entered only now
        tx.send(()).unwrap();
        rx2.recv().unwrap();
        first
    });
    rx.recv().unwrap();
    println!("thread started: VmSize {:+} KiB", vmsize_kib() as i64 - v0 as i64);
    p.report("thread started (idle)");
    tx2.send(()).unwrap();
    rx.recv().unwrap();
    p.report("thread entered a function with a 1 MiB frame");
    tx2.send(()).unwrap();
    t.join().unwrap();
    p.report("thread joined (stack unmapped)");
}
