// verify: release ok
//! The blocking pool: separate threads, created on demand up to max_blocking_threads (default 512),
//! kept alive for thread_keep_alive (default 10 s) after they go idle. spawn_blocking can't be aborted.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn thread_count() -> usize {
    std::fs::read_dir("/proc/self/task").unwrap().count()
}

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(8)
        .thread_keep_alive(Duration::from_millis(300))
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        println!("threads at start: {} (main + 2 workers)", thread_count());

        // 32 jobs of 100 ms each on at most 8 blocking threads: 4 waves.
        let t = Instant::now();
        let jobs: Vec<_> = (0..32).map(|_| tokio::task::spawn_blocking(|| std::thread::sleep(Duration::from_millis(100)))).collect();
        tokio::time::sleep(Duration::from_millis(50)).await;
        println!("threads while 32 jobs run: {}", thread_count());
        for j in jobs {
            j.await.unwrap();
        }
        println!("32 x 100 ms on max_blocking_threads(8) took {:.0?}", t.elapsed());
        tokio::time::sleep(Duration::from_millis(600)).await;
        println!("threads after 600 ms idle (keep-alive 300 ms): {}", thread_count());

        // abort() can't interrupt a closure that is already running on a blocking thread.
        let finished = Arc::new(AtomicBool::new(false));
        let f = Arc::clone(&finished);
        let h = tokio::task::spawn_blocking(move || {
            std::thread::sleep(Duration::from_millis(100));
            f.store(true, Ordering::SeqCst);
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        h.abort();
        let r = h.await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        println!("aborted spawn_blocking: join result ok = {}, closure ran to the end = {}", r.is_ok(), finished.load(Ordering::SeqCst));
    });
}
