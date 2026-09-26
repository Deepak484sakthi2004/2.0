// verify: debug ok
// A tour of this process's virtual address space: every mapping from /proc/self/maps, where typical Rust values live,
// and the canonical-address rule (on x86-64 with 4-level paging, user addresses fit in 47 bits).
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);
static GREETING: &str = "hello";

struct Region {
    lo: u64,
    hi: u64,
    perms: String,
    name: String,
}

fn regions() -> Vec<Region> {
    std::fs::read_to_string("/proc/self/maps")
        .unwrap()
        .lines()
        .map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            let (lo, hi) = f[0].split_once('-').unwrap();
            Region {
                lo: u64::from_str_radix(lo, 16).unwrap(),
                hi: u64::from_str_radix(hi, 16).unwrap(),
                perms: f[1].to_string(),
                name: f.get(5).map_or("[anonymous]".to_string(), |n| n.rsplit('/').next().unwrap().to_string()),
            }
        })
        .collect()
}

fn where_is(addr: u64, rs: &[Region]) -> String {
    rs.iter().find(|r| (r.lo..r.hi).contains(&addr)).map_or("not mapped".into(), |r| format!("{} {}", r.perms, r.name))
}

fn status(field: &str) -> String {
    let s = std::fs::read_to_string("/proc/self/status").unwrap();
    s.lines().find(|l| l.starts_with(field)).unwrap().split_whitespace().skip(1).collect::<Vec<_>>().join(" ")
}

fn main() {
    COUNTER.fetch_add(1, Ordering::Relaxed);
    let small = Box::new([1u8; 64]); // from the heap arena (brk)
    let large = vec![1u8; 1 << 20]; // above glibc's mmap threshold: its own mapping
    let local = 42u64;
    let (tx, rx) = std::sync::mpsc::channel();
    let t = std::thread::spawn(move || {
        let on_thread_stack = 7u64;
        tx.send(&on_thread_stack as *const u64 as u64).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
    });
    let thread_local_addr = rx.recv().unwrap();
    let rs = regions();

    println!("--- {} mappings ---", rs.len());
    for r in &rs {
        println!("{:#014x}-{:#014x} {:>9} KiB {} {}", r.lo, r.hi, (r.hi - r.lo) / 1024, r.perms, r.name);
    }
    // SAFETY: getauxval has no preconditions.
    let vdso = unsafe { libc::getauxval(libc::AT_SYSINFO_EHDR) };
    let things: [(&str, u64); 10] = [
        ("fn main", main as *const () as u64),
        ("static COUNTER", &COUNTER as *const AtomicU64 as u64),
        ("string literal", GREETING.as_ptr() as u64),
        ("Box<[u8; 64]>", small.as_ptr() as u64),
        ("vec![0; 1 MiB]", large.as_ptr() as u64),
        ("local on main stack", &local as *const u64 as u64),
        ("local on thread stack", thread_local_addr),
        ("libc::getpid", libc::getpid as *const () as u64),
        ("vDSO", vdso),
        ("1 << 47 (limit)", 1u64 << 47),
    ];
    println!("--- where things live ---");
    for (what, addr) in things {
        let canonical = addr >> 47 == 0; // user half of the 4-level-paging address space
        println!("{what:<22} {addr:#014x}  bits used: {:>2}  {:<10} {}", 64 - addr.leading_zeros(),
            if canonical { "user half" } else { "NOT user" }, where_is(addr, &rs));
    }
    println!("--- the kernel's summary ---");
    for f in ["VmSize", "VmRSS", "RssAnon", "RssFile", "VmData", "VmStk", "VmExe", "VmLib", "VmPTE", "Threads"] {
        println!("{f:<8} {}", status(f));
    }
    t.join().unwrap();
    assert!(small[0] == 1 && large[0] == 1);
}
