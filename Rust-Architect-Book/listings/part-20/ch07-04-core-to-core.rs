// verify: release ok
// How far apart are two cores? Two threads bounce one cache line back and forth (ping-pong on an atomic), pinned to
// chosen CPUs with sched_setaffinity. The round trip is the cost of moving a line between those cores' caches.
// The same experiment shows the topology this process sees (NUMA nodes, SMT siblings). One Playground run, noisy.
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering::{Acquire, Release}};
use std::thread;
use std::time::Instant;

fn read(path: &str) -> String {
    std::fs::read_to_string(path).map(|s| s.trim().to_string()).unwrap_or_else(|_| "unavailable".into())
}

fn pin(cpu: usize) -> bool {
    // SAFETY: cpu_set_t is a plain bitmask; CPU_ZERO/CPU_SET only write into the local set, and
    // sched_setaffinity(0, ...) applies it to the calling thread.
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_ZERO(&mut set);
        libc::CPU_SET(cpu, &mut set);
        libc::sched_setaffinity(0, size_of::<libc::cpu_set_t>(), &set) == 0
    }
}

fn current_cpu() -> i32 {
    // SAFETY: sched_getcpu has no preconditions.
    unsafe { libc::sched_getcpu() }
}

/// Mean round-trip ns between a thread on `a` and a thread on `b` (None = not pinned).
fn ping_pong(a: Option<usize>, b: Option<usize>) -> (f64, bool) {
    let flag = Arc::new(AtomicU64::new(0));
    let rounds = 200_000u64;
    let f2 = flag.clone();
    let ponger = thread::spawn(move || {
        let ok = b.map_or(true, pin);
        for i in 0..rounds {
            while f2.load(Acquire) != 2 * i + 1 {
                std::hint::spin_loop();
            }
            f2.store(2 * i + 2, Release);
        }
        ok
    });
    let ok = a.map_or(true, pin);
    let t = Instant::now();
    for i in 0..rounds {
        flag.store(2 * i + 1, Release);
        while flag.load(Acquire) != 2 * i + 2 {
            std::hint::spin_loop();
        }
    }
    let ns = t.elapsed().as_nanos() as f64 / rounds as f64;
    let ok2 = ponger.join().unwrap();
    (ns, ok && ok2)
}

fn main() {
    let cpus = thread::available_parallelism().map_or(1, |n| n.get());
    println!("NUMA nodes online: {}", read("/sys/devices/system/node/online"));
    for c in 0..cpus {
        println!(
            "cpu{c}: core_id {}, package {}, SMT siblings {}",
            read(&format!("/sys/devices/system/cpu/cpu{c}/topology/core_id")),
            read(&format!("/sys/devices/system/cpu/cpu{c}/topology/physical_package_id")),
            read(&format!("/sys/devices/system/cpu/cpu{c}/topology/thread_siblings_list"))
        );
    }
    println!("main thread starts on cpu {}", current_cpu());
    let (ns, _) = ping_pong(None, None);
    println!("round trip, not pinned:        {ns:6.1} ns");
    for b in 1..cpus {
        let (ns, ok) = ping_pong(Some(0), Some(b));
        println!("round trip, cpu0 <-> cpu{b}:      {ns:6.1} ns{}", if ok { "" } else { "   (pinning refused)" });
    }
}
