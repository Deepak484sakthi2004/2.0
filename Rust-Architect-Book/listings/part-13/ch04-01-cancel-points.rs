// verify: debug ok
//! Cancellation = the future is dropped. It stops at the .await where it was suspended: code after that
//! .await never runs, and destructors of everything it owned do run. Paused clock: exact timings.
use std::time::Duration;
use tokio::time::{sleep, timeout, Instant};

struct Reporter(&'static str);
impl Drop for Reporter {
    fn drop(&mut self) {
        println!("    drop({})", self.0);
    }
}

async fn transfer(log: &mut Vec<&'static str>) -> &'static str {
    let _debit_guard = Reporter("debit guard");
    log.push("debited source");
    sleep(Duration::from_millis(40)).await; // .await #1: call the ledger
    log.push("credited destination");
    sleep(Duration::from_millis(40)).await; // .await #2: notify the merchant
    log.push("notified");
    "done"
}

#[tokio::main(flavor = "current_thread", start_paused = true)]
async fn main() {
    for budget in [100, 60, 20] {
        let mut log = Vec::new();
        let start = Instant::now();
        println!("timeout({budget} ms):");
        let r = timeout(Duration::from_millis(budget), transfer(&mut log)).await;
        println!("    result {:?} after {:?}, steps that ran: {log:?}", r.map_err(|_| "Elapsed"), start.elapsed());
    }

    // abort() on a spawned task is the same thing: the runtime drops the future at its next suspension.
    let h = tokio::spawn(async {
        let _g = Reporter("spawned task's guard");
        sleep(Duration::from_secs(60)).await;
        println!("    never printed");
    });
    tokio::task::yield_now().await; // let it start and suspend
    h.abort();
    println!("abort(): join result is_cancelled = {}", h.await.unwrap_err().is_cancelled());
}
