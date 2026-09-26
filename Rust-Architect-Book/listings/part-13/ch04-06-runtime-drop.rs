// verify: debug ok
//! When main returns, the runtime is dropped, and every task still running is dropped at its current
//! .await: detached work simply stops. Blocking tasks can't be stopped: shutdown_timeout bounds the wait.
use std::time::{Duration, Instant};

struct Reporter(&'static str);
impl Drop for Reporter {
    fn drop(&mut self) {
        println!("  drop({})", self.0);
    }
}

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    rt.block_on(async {
        tokio::spawn(async {
            let _r = Reporter("audit-upload task state");
            println!("  audit upload: part 1 sent");
            tokio::time::sleep(Duration::from_millis(200)).await;
            println!("  audit upload: part 2 sent"); // never printed: the runtime is gone by then
        });
        tokio::task::spawn_blocking(|| {
            std::thread::sleep(Duration::from_millis(500));
            println!("  blocking export finished"); // runs to the end on its own thread, detached
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        println!("main's work is done; returning");
    });
    let t = Instant::now();
    rt.shutdown_timeout(Duration::from_millis(100)); // bounded: stop waiting for blocking work after 100 ms
    println!("runtime shut down after {:?} (didn't wait for the blocking task)", t.elapsed());
    std::thread::sleep(Duration::from_millis(600)); // keep the process alive long enough to see the detached thread

    // A plain drop (what #[tokio::main] does when main returns) waits for running blocking tasks.
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).build().unwrap();
    rt.spawn_blocking(|| std::thread::sleep(Duration::from_millis(300)));
    std::thread::sleep(Duration::from_millis(20)); // let the blocking closure start
    let t = Instant::now();
    drop(rt);
    println!("plain drop of a runtime with a 300 ms blocking task took {:?}", t.elapsed());
}
