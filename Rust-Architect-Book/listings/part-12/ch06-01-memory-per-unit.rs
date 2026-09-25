// verify: release ok
//! Memory per unit of concurrency: idle OS threads (each blocked in recv) vs idle async tasks
//! (each awaiting a oneshot). Threads: /proc/self/status. Tasks: the counting allocator and RSS.
//! One run; RSS numbers depend on the allocator and kernel [OS][LIB].
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards to the system allocator unchanged; tracks live requested bytes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn kb(field: &str) -> i64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap();
    let line = s.lines().find(|l| l.starts_with(field)).unwrap();
    line.split_whitespace().nth(1).unwrap().parse().unwrap()
}

fn main() {
    // --- OS threads: spawn 50 first (glibc creates its per-thread malloc arenas early),
    //     then measure 400 more.
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let rx = std::sync::Arc::new(std::sync::Mutex::new(rx));
    let spawn_idle = |n: usize| -> Vec<std::thread::JoinHandle<()>> {
        (0..n)
            .map(|_| {
                let rx = rx.clone();
                std::thread::spawn(move || {
                    let _ = rx.lock().unwrap().recv(); // blocks until the channel closes
                })
            })
            .collect()
    };
    let mut threads = spawn_idle(50);
    std::thread::sleep(std::time::Duration::from_millis(50));
    let (vsz0, rss0) = (kb("VmSize"), kb("VmRSS"));
    threads.extend(spawn_idle(400));
    std::thread::sleep(std::time::Duration::from_millis(100));
    let (vsz1, rss1) = (kb("VmSize"), kb("VmRSS"));
    println!("idle OS thread:  {:>6} KiB virtual, {:>5.1} KiB resident each", (vsz1 - vsz0) / 400, (rss1 - rss0) as f64 / 400.0);
    drop(tx);
    for t in threads {
        t.join().unwrap();
    }

    // --- async tasks: 100,000 of them, each suspended on a oneshot that is still open.
    use futures::task::LocalSpawnExt;
    const N: usize = 100_000;
    let mut pool = futures::executor::LocalPool::new();
    let (live0, rss0) = (LIVE.load(Ordering::Relaxed) as i64, kb("VmRSS"));
    let mut senders = Vec::with_capacity(N);
    for i in 0..N {
        let (tx, rx) = futures::channel::oneshot::channel::<u64>();
        senders.push(tx);
        pool.spawner()
            .spawn_local(async move {
                let v = rx.await.unwrap_or(0); // a connection waiting for its next message
                std::hint::black_box(v + i as u64);
            })
            .unwrap();
    }
    pool.run_until_stalled(); // poll each task once: all are now suspended
    let senders_bytes = (senders.capacity() * size_of::<futures::channel::oneshot::Sender<u64>>()) as i64;
    let live1 = LIVE.load(Ordering::Relaxed) as i64 - senders_bytes; // don't count our Vec of senders
    let rss1 = kb("VmRSS");
    println!(
        "idle async task: {:>6.0} B heap (task + future + oneshot), {:>5.2} KiB resident each",
        (live1 - live0) as f64 / N as f64,
        (rss1 - rss0) as f64 / N as f64
    );
    drop(senders); // every oneshot resolves with Err: the tasks finish
    pool.run();
    println!("after completion: {} B still allocated", LIVE.load(Ordering::Relaxed) as i64 - live0);

    // Same again: if that was a leak it would double; if it's retained capacity, it's reused.
    for i in 0..N {
        pool.spawner().spawn_local(async move { std::hint::black_box(i); }).unwrap();
    }
    pool.run();
    println!("after a second 100,000 tasks: {} B still allocated", LIVE.load(Ordering::Relaxed) as i64 - live0);
}
