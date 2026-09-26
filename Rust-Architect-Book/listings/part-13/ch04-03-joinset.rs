// verify: debug ok
//! JoinSet: a scope for spawned tasks. Results arrive in completion order; dropping the set aborts
//! whatever is still running, so no task outlives the code that owns the set. Paused clock.
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time::{sleep, Instant};

struct Reporter(u32);
impl Drop for Reporter {
    fn drop(&mut self) {
        println!("    task {} dropped", self.0);
    }
}

async fn quote(provider: u32, ms: u64) -> (u32, u64) {
    let _r = Reporter(provider);
    sleep(Duration::from_millis(ms)).await;
    if provider == 3 {
        panic!("provider 3 sent a malformed quote");
    }
    (provider, ms)
}

#[tokio::main(flavor = "current_thread", start_paused = true)]
async fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let start = Instant::now();
    {
        let mut set = JoinSet::new();
        for (provider, ms) in [(1, 120), (2, 40), (3, 60), (4, 500), (5, 80)] {
            set.spawn(quote(provider, ms));
        }
        // Take the first two good quotes, then stop caring about the rest.
        let mut good = Vec::new();
        while let Some(res) = set.join_next().await {
            match res {
                Ok(q) => good.push(q),
                Err(e) if e.is_panic() => println!("  at {:>3?}: a quote task panicked", start.elapsed()),
                Err(e) => println!("  at {:>3?}: {e}", start.elapsed()),
            }
            if good.len() == 2 {
                break;
            }
        }
        println!("  at {:>3?}: using {good:?}; {} tasks still running; dropping the JoinSet", start.elapsed(), set.len());
    } // <- the JoinSet is dropped here: every remaining task is aborted
    tokio::task::yield_now().await;
    println!("  at {:>3?}: done", start.elapsed());
}
