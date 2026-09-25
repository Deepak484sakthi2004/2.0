// verify: debug ok
//! The same "wait for a signal" future, three ways. Two of them hang forever, and nothing
//! crashes: the executor is parked, waiting for a wake that never comes.
use std::future::Future;
use std::pin::Pin;
use std::sync::{mpsc, Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Shared {
    done: bool,
    waker: Option<Waker>,
}

#[derive(Clone, Copy, Debug)]
enum Strategy {
    NeverRegisters,
    RegistersOnce,
    RefreshesEveryPoll,
}

struct Signal {
    shared: Arc<Mutex<Shared>>,
    strategy: Strategy,
}

impl Future for Signal {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut s = self.shared.lock().unwrap();
        if s.done {
            return Poll::Ready(());
        }
        match self.strategy {
            // BUG: Pending, and nobody will ever call wake.
            Strategy::NeverRegisters => {}
            // BUG: keeps the waker from the first poll, even if a different task polls us later.
            Strategy::RegistersOnce => {
                if s.waker.is_none() {
                    s.waker = Some(cx.waker().clone());
                }
            }
            // Correct: every poll leaves the CURRENT waker behind (skipping the clone if it's the same one).
            Strategy::RefreshesEveryPoll => match &mut s.waker {
                Some(w) if w.will_wake(cx.waker()) => {}
                slot => *slot = Some(cx.waker().clone()),
            },
        }
        Poll::Pending
    }
}

fn complete(shared: &Mutex<Shared>) {
    let mut s = shared.lock().unwrap();
    s.done = true;
    if let Some(w) = s.waker.take() {
        w.wake();
    }
}

fn run(strategy: Strategy) {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let mut fut = Signal { shared: shared.clone(), strategy };
    // The first poll happens somewhere else (think: inside a select! in another task), with another waker.
    let _ = Pin::new(&mut fut).poll(&mut Context::from_waker(Waker::noop()));

    let (finished_tx, finished_rx) = mpsc::channel();
    let start = Instant::now();
    std::thread::spawn(move || {
        futures::executor::block_on(fut); // now a real executor owns the future
        finished_tx.send(()).unwrap();
    });
    std::thread::sleep(Duration::from_millis(20));
    complete(&shared); // the event the future is waiting for happens

    match finished_rx.recv_timeout(Duration::from_millis(500)) {
        Ok(()) => println!("{strategy:?}: completed after ~{} ms", start.elapsed().as_millis() / 10 * 10),
        Err(_) => {
            let done = shared.lock().unwrap().done;
            println!("{strategy:?}: HUNG (done = {done}, but the executor was never woken)");
        }
    }
}

fn main() {
    run(Strategy::NeverRegisters);
    run(Strategy::RegistersOnce);
    run(Strategy::RefreshesEveryPoll);
}
