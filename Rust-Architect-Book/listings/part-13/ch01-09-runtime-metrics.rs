// verify: debug ok
//! The runtime metrics available on stable tokio (1.53.1): enough for gauges and saturation alerts.
//! Steal counts, poll-time histograms and friends need RUSTFLAGS="--cfg tokio_unstable" (listing ch01-10).
#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let m = tokio::runtime::Handle::current().metrics();
    println!("workers {} alive {} global_queue_depth {}", m.num_workers(), m.num_alive_tasks(), m.global_queue_depth());
    println!("busy {:?} parks {} park_unpark {}", m.worker_total_busy_duration(0), m.worker_park_count(0), m.worker_park_unpark_count(0));
    tokio::task::coop::consume_budget().await; // spend one unit of the cooperative budget (a yield point)
    println!("TOKIO_WORKER_THREADS: {:?}", std::env::var("TOKIO_WORKER_THREADS"));
}
