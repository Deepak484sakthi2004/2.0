// verify: debug ok
//! One-time initialization, raced by 8 threads: OnceLock runs the initializer once and everyone sees it.
//! LazyLock (stable since 1.80) packages the same thing for statics. What happens when initialization panics?
use std::collections::HashMap;
use std::panic;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{LazyLock, OnceLock};
use std::thread;

static INIT_RUNS: AtomicU32 = AtomicU32::new(0);
static PRICE_TABLE: OnceLock<HashMap<&'static str, u64>> = OnceLock::new();

fn prices() -> &'static HashMap<&'static str, u64> {
    PRICE_TABLE.get_or_init(|| {
        INIT_RUNS.fetch_add(1, Ordering::Relaxed);
        thread::sleep(std::time::Duration::from_millis(20)); // a slow load: the race window is wide open
        HashMap::from([("basic", 900), ("pro", 2900)])
    })
}

static REGION: LazyLock<String> = LazyLock::new(|| std::env::var("REGION").unwrap_or_else(|_| "eu-west-1".into()));

static BROKEN: LazyLock<u32> = LazyLock::new(|| panic!("config file missing"));

fn main() {
    panic::set_hook(Box::new(|info| println!("  [panic] {}", info.payload_as_str().unwrap_or("?"))));

    let seen: Vec<u64> = thread::scope(|s| {
        let hs: Vec<_> = (0..8).map(|_| s.spawn(|| prices()["pro"])).collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    println!("8 threads saw {:?}; initializer ran {} time(s)", seen, INIT_RUNS.load(Ordering::Relaxed));
    println!("REGION = {}", *REGION);

    // A panicking initializer: OnceLock stays empty (a later call may retry), LazyLock is poisoned for good.
    let once: OnceLock<u32> = OnceLock::new();
    let r = panic::catch_unwind(|| *once.get_or_init(|| panic!("first attempt failed")));
    println!("OnceLock after a panicking init: is_err = {}, get() = {:?}", r.is_err(), once.get());
    println!("OnceLock retry: {}", once.get_or_init(|| 7));

    for attempt in 1..=2 {
        let r = panic::catch_unwind(|| *BROKEN);
        println!("LazyLock access #{attempt}: panicked = {}", r.is_err());
    }
}
