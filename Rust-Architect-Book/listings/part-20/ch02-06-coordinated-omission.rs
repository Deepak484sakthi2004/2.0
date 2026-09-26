// verify: release ok
// Coordinated omission, simulated in virtual time (deterministic, no wall clock involved).
// A server answers every request in 1 ms, except that it freezes for 2 s once (a GC pause, a lock, a disk stall).
// The load test intends to send one request every 2 ms (500 req/s) for 100 s.
use hdrhistogram::Histogram;

const SERVICE_US: u64 = 1_000;
const INTERVAL_US: u64 = 2_000;
const DURATION_US: u64 = 100_000_000;
const STALL_START_US: u64 = 30_000_000;
const STALL_US: u64 = 2_000_000;

/// When does the server finish a request that starts service at `start_us`?
fn finish(start_us: u64) -> u64 {
    if start_us < STALL_START_US + STALL_US && start_us + SERVICE_US > STALL_START_US {
        start_us.max(STALL_START_US) + STALL_US + SERVICE_US // caught by the stall
    } else {
        start_us + SERVICE_US
    }
}

fn report(label: &str, h: &Histogram<u64>) {
    println!(
        "{label:<44} n={:>6}  p50 {:>7.1} ms  p99 {:>7.1} ms  p99.9 {:>7.1} ms  max {:>7.1} ms",
        h.len(),
        h.value_at_percentile(50.0) as f64 / 1e3,
        h.value_at_percentile(99.0) as f64 / 1e3,
        h.value_at_percentile(99.9) as f64 / 1e3,
        h.max() as f64 / 1e3
    );
}

fn main() {
    let new = || Histogram::<u64>::new_with_bounds(1, 60_000_000, 3).unwrap();

    // 1. Closed loop (what many load tools do on one connection): send, wait for the reply,
    //    then send the next one at the next 2 ms tick (or immediately, if the reply was late).
    let mut closed = new();
    let mut t = 0;
    while t < DURATION_US {
        let done = finish(t);
        closed.record(done - t).unwrap();
        t = (t + INTERVAL_US).max(done); // the tool silently skips the ticks it missed
    }

    // 2. Open loop: every request is sent at its intended time, whatever happened before;
    //    the server queues them (FIFO), and latency is measured from the intended send time.
    let mut open = new();
    let mut server_free = 0;
    let mut intended = 0;
    while intended < DURATION_US {
        let start = intended.max(server_free);
        let done = finish(start);
        server_free = done;
        open.record(done - intended).unwrap();
        intended += INTERVAL_US;
    }

    // 3. The closed-loop data, corrected afterwards with HdrHistogram's expected-interval correction.
    let mut corrected = new();
    let mut t = 0;
    while t < DURATION_US {
        let done = finish(t);
        corrected.record_correct(done - t, INTERVAL_US).unwrap();
        t = (t + INTERVAL_US).max(done);
    }

    report("closed loop (reply-paced client)", &closed);
    report("open loop (latency from intended send time)", &open);
    report("closed loop + record_correct(interval)", &corrected);
    println!(
        "requests the closed-loop client never sent during the stall: {}",
        open.len() - closed.len()
    );
}
