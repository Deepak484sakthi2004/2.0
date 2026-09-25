// verify: debug ok
// verify: release ok
//! A complete (small) executor: a run queue of tasks, wakers that re-queue their task, a timer
//! thread, and JoinHandles. Single worker thread; about 150 lines.
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

// ---------- tasks and the run queue ----------

type BoxFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

struct Task {
    future: Mutex<Option<BoxFuture>>, // None once finished: dropping the future frees its state
    queue: Sender<Arc<Task>>,
    queued: AtomicBool, // at most one queue entry per task, however many times it's woken
}

impl Wake for Task {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        if !self.queued.swap(true, Ordering::AcqRel) {
            let _ = self.queue.send(self.clone()); // fails only if the executor is gone
        }
    }
}

static POLLS: AtomicU32 = AtomicU32::new(0);

#[derive(Clone)]
struct Spawner(Sender<Arc<Task>>);

struct Executor(Receiver<Arc<Task>>);

fn executor() -> (Executor, Spawner) {
    let (tx, rx) = mpsc::channel();
    (Executor(rx), Spawner(tx))
}

impl Executor {
    /// Runs until every task has finished and every waker is gone (all Senders dropped).
    fn run(self) {
        while let Ok(task) = self.0.recv() {
            task.queued.store(false, Ordering::Release); // a wake during the poll re-queues it
            let waker = Waker::from(task.clone());
            let mut cx = Context::from_waker(&waker);
            let mut slot = task.future.lock().unwrap();
            if let Some(fut) = slot.as_mut() {
                POLLS.fetch_add(1, Ordering::Relaxed);
                if fut.as_mut().poll(&mut cx).is_ready() {
                    *slot = None; // drop the finished future now, not when the last waker dies
                }
            }
        }
    }
}

// ---------- JoinHandle: a future for another task's result ----------

struct JoinState<T> {
    result: Option<T>,
    waker: Option<Waker>,
}

struct JoinHandle<T>(Arc<Mutex<JoinState<T>>>);

impl<T> Future for JoinHandle<T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let mut st = self.0.lock().unwrap();
        match st.result.take() {
            Some(v) => Poll::Ready(v),
            None => {
                st.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl Spawner {
    fn spawn<F>(&self, fut: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let state = Arc::new(Mutex::new(JoinState { result: None, waker: None }));
        let shared = state.clone();
        let wrapped = async move {
            let out = fut.await;
            let waker = {
                let mut st = shared.lock().unwrap();
                st.result = Some(out);
                st.waker.take()
            };
            if let Some(w) = waker {
                w.wake();
            }
        };
        let task = Arc::new(Task {
            future: Mutex::new(Some(Box::pin(wrapped))),
            queue: self.0.clone(),
            queued: AtomicBool::new(true),
        });
        self.0.send(task).unwrap();
        JoinHandle(state)
    }
}

// ---------- the timer: one thread, a min-heap of deadlines ----------

struct TimerEntry {
    deadline: Instant,
    seq: u64,
    shared: Arc<Mutex<SleepShared>>,
}
impl PartialEq for TimerEntry {
    fn eq(&self, other: &Self) -> bool {
        (self.deadline, self.seq) == (other.deadline, other.seq)
    }
}
impl Eq for TimerEntry {}
impl PartialOrd for TimerEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for TimerEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.deadline, self.seq).cmp(&(other.deadline, other.seq))
    }
}

#[derive(Default)]
struct SleepShared {
    fired: bool,
    waker: Option<Waker>,
}

struct Timer {
    heap: Mutex<(BinaryHeap<Reverse<TimerEntry>>, u64)>,
    changed: Condvar,
}

fn timer() -> &'static Timer {
    static TIMER: OnceLock<&'static Timer> = OnceLock::new();
    TIMER.get_or_init(|| {
        let t: &'static Timer = Box::leak(Box::new(Timer { heap: Mutex::default(), changed: Condvar::new() }));
        std::thread::spawn(move || loop {
            let mut guard = t.heap.lock().unwrap();
            let now = Instant::now();
            while guard.0.peek().is_some_and(|Reverse(e)| e.deadline <= now) {
                let Reverse(e) = guard.0.pop().unwrap();
                let mut s = e.shared.lock().unwrap();
                s.fired = true;
                if let Some(w) = s.waker.take() {
                    w.wake();
                }
            }
            let wait = guard.0.peek().map(|Reverse(e)| e.deadline - now).unwrap_or(Duration::from_secs(3600));
            drop(t.changed.wait_timeout(guard, wait).unwrap());
        });
        t
    })
}

struct Sleep {
    deadline: Instant,
    shared: Arc<Mutex<SleepShared>>,
    registered: bool,
}

fn sleep(d: Duration) -> Sleep {
    Sleep { deadline: Instant::now() + d, shared: Arc::default(), registered: false }
}

impl Future for Sleep {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        {
            let mut s = self.shared.lock().unwrap();
            if s.fired {
                return Poll::Ready(());
            }
            s.waker = Some(cx.waker().clone()); // refresh on every poll (Chapter 12.2)
        }
        if !self.registered {
            self.registered = true;
            let t = timer();
            let mut guard = t.heap.lock().unwrap();
            guard.1 += 1;
            let entry = TimerEntry { deadline: self.deadline, seq: guard.1, shared: self.shared.clone() };
            guard.0.push(Reverse(entry));
            t.changed.notify_one(); // the new deadline may be the earliest
        }
        Poll::Pending
    }
}

// ---------- demo ----------

fn main() {
    let (exec, spawner) = executor();
    let start = Instant::now();
    let log = Arc::new(Mutex::new(Vec::<String>::new()));

    let s = spawner.clone();
    let log2 = log.clone();
    spawner.spawn(async move {
        let mut handles = Vec::new();
        for (name, ms) in [("settlement", 30), ("fx-rates", 10), ("fraud-model", 20)] {
            let log = log2.clone();
            handles.push(s.spawn(async move {
                sleep(Duration::from_millis(ms)).await;
                log.lock().unwrap().push(format!("{name} done"));
                format!("{name} refreshed")
            }));
        }
        for h in handles {
            let msg = h.await; // awaited in spawn order; they ran concurrently
            log2.lock().unwrap().push(msg);
        }
    });
    drop(spawner); // the executor stops once no one can spawn or wake anything
    exec.run();

    let elapsed = start.elapsed().as_millis() / 10 * 10;
    println!("log: {:?}", log.lock().unwrap());
    println!("4 tasks, {} polls, ~{elapsed} ms total (sleeps of 30 + 10 + 20 ms)", POLLS.load(Ordering::Relaxed));
}
