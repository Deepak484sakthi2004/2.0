// verify: debug ok
//! Read Tokio's defaults from Tokio's own source, on the machine that compiled this program.
//! The Playground keeps the registry sources next to the build, so a program can grep its dependencies.
//! Everything printed here is an implementation detail of tokio 1.53.1 [LIB], not a promise.
use std::fs;

const ROOT: &str = "/playground/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tokio-1.53.1/src";

/// Print every line of `file` that contains one of `needles`, with its line number.
fn show(file: &str, needles: &[&str]) {
    let text = fs::read_to_string(format!("{ROOT}/{file}")).expect("tokio source not found");
    for (i, line) in text.lines().enumerate() {
        if needles.iter().any(|n| line.contains(n)) {
            println!("{file}:{}: {}", i + 1, line.trim());
        }
    }
}

/// Print the line containing `needle` and the `n` lines after it.
fn show_after(file: &str, needle: &str, n: usize) {
    let text = fs::read_to_string(format!("{ROOT}/{file}")).expect("tokio source not found");
    let lines: Vec<&str> = text.lines().collect();
    let at = lines.iter().position(|l| l.contains(needle)).expect("needle not found");
    for (i, line) in lines.iter().enumerate().skip(at).take(n + 1) {
        println!("{file}:{}: {}", i + 1, line.trim());
    }
}

fn main() {
    show("task/coop/mod.rs", &["Budget(Some("]); // the cooperative budget per task poll
    show_after("runtime/mod.rs", "const BOX_FUTURE_THRESHOLD", 4); // spawn boxes futures bigger than this first
    show("runtime/scheduler/multi_thread/queue.rs", &["const LOCAL_QUEUE_CAPACITY: usize = 256"]); // per-worker run queue
    show("runtime/scheduler/multi_thread/worker.rs", &["const MAX_LIFO_POLLS_PER_TICK"]); // LIFO slot limit
    show("runtime/builder.rs", &["Builder::new(Kind::MultiThread, 61)", "max_blocking_threads: 512", "nevents: 1024"]);
    show("runtime/blocking/pool.rs", &["const KEEP_ALIVE"]); // idle blocking threads live this long
    show("runtime/time/wheel/level.rs", &["const LEVEL_MULT"]); // slots per timer-wheel level
    show("runtime/time/wheel/mod.rs", &["const NUM_LEVELS", "const MAX_DURATION"]);
}
