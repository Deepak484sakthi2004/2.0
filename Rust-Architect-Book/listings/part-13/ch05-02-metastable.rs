// verify: debug ok
//! Overload with and without admission control, simulated with tokio on a paused (virtual) clock.
//! A server handles one request at a time, 10 ms each (100 requests/s). Clients send 120 requests/s for
//! 10 s and give up after 500 ms. "Goodput" = replies that reached a client that was still waiting.
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{sleep, timeout, Instant};

struct Request {
    deadline: Instant,
    reply: oneshot::Sender<()>,
}

#[derive(Clone, Copy, PartialEq)]
enum Policy {
    UnboundedQueue,
    BoundedQueueShed(usize),
    SkipIfCannotFinish,
}

async fn run(policy: Policy) {
    let start = Instant::now();
    let (tx, mut rx) = mpsc::unbounded_channel::<Request>();
    let queued = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let q = queued.clone();
    let server = tokio::spawn(async move {
        let (mut worked, mut wasted, mut skipped) = (0u32, 0u32, 0u32);
        while let Some(req) = rx.recv().await {
            q.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            if policy == Policy::SkipIfCannotFinish && Instant::now() + Duration::from_millis(10) > req.deadline {
                skipped += 1; // it would finish after the client gave up: don't do the work
                continue;
            }
            sleep(Duration::from_millis(10)).await; // the work
            worked += 1;
            if req.reply.send(()).is_err() {
                wasted += 1; // the client had already given up
            }
        }
        (worked, wasted, skipped, start.elapsed())
    });

    let (mut clients, mut shed) = (Vec::new(), 0u32);
    for i in 0..1_200u32 {
        let arrive = start + Duration::from_micros(i as u64 * 8_333); // 120 per second
        tokio::time::sleep_until(arrive).await;
        if let Policy::BoundedQueueShed(cap) = policy {
            if queued.load(std::sync::atomic::Ordering::SeqCst) >= cap {
                shed += 1; // refuse now: "busy, retry later" costs the server nothing
                continue;
            }
        }
        let (reply, rx) = oneshot::channel();
        queued.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        tx.send(Request { deadline: arrive + Duration::from_millis(500), reply }).unwrap();
        clients.push(tokio::spawn(async move {
            let t = Instant::now();
            match timeout(Duration::from_millis(500), rx).await {
                Ok(Ok(())) => Some(t.elapsed()), // a reply, in time
                Ok(Err(_)) => None, // the server dropped the request without replying (skipped)
                Err(_) => None, // gave up after 500 ms
            }
        }));
    }
    drop(tx);
    let mut ok_latencies: Vec<Duration> = Vec::new();
    for c in clients {
        if let Some(lat) = c.await.unwrap() {
            ok_latencies.push(lat);
        }
    }
    let (worked, wasted, skipped, busy_until) = server.await.unwrap();
    ok_latencies.sort();
    let p50 = ok_latencies.get(ok_latencies.len() / 2).copied().unwrap_or_default();
    let name = match policy {
        Policy::UnboundedQueue => "unbounded queue".to_string(),
        Policy::BoundedQueueShed(c) => format!("bounded queue ({c}), shed the rest"),
        Policy::SkipIfCannotFinish => "unbounded, skip if it can't finish".to_string(),
    };
    println!(
        "{name:<36} goodput {:>4}   shed {shed:>3}   work done {worked:>4} (wasted {wasted:>4})   skipped {skipped:>3}   p50 ok latency {p50:>6.0?}   server busy until {busy_until:.1?}",
        ok_latencies.len()
    );
}

#[tokio::main(flavor = "current_thread", start_paused = true)]
async fn main() {
    run(Policy::UnboundedQueue).await;
    run(Policy::BoundedQueueShed(20)).await;
    run(Policy::SkipIfCannotFinish).await;
}
