// verify: release ok
//! What a task costs: allocations, bytes and time per spawned task, on this chapter's executor
//! and on futures' LocalPool, next to spawning and joining an OS thread.
//! One run on a shared machine: the times are noisy; the allocation counts are exact.
use std::alloc::{GlobalAlloc, Layout, System};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Instant;

struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards to the system allocator unchanged; only counts calls and requested bytes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

struct YieldOnce(bool);
impl Future for YieldOnce {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            return Poll::Ready(());
        }
        self.0 = true;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

/// The task body: a request handler that suspends once.
async fn handler(id: u64) -> u64 {
    YieldOnce(false).await;
    id * 2
}

// --- this chapter's executor, reduced to its core (see ch05-03) ---
type BoxFuture = Pin<Box<dyn Future<Output = ()> + Send>>;
struct Task {
    future: Mutex<Option<BoxFuture>>,
    queue: Sender<Arc<Task>>,
    queued: AtomicBool,
}
impl Wake for Task {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        if !self.queued.swap(true, Ordering::AcqRel) {
            let _ = self.queue.send(self.clone());
        }
    }
}

fn run_ours(n: u64) {
    let (tx, rx) = mpsc::channel::<Arc<Task>>();
    for id in 0..n {
        let fut: BoxFuture = Box::pin(async move {
            std::hint::black_box(handler(id).await);
        });
        let task = Arc::new(Task { future: Mutex::new(Some(fut)), queue: tx.clone(), queued: AtomicBool::new(true) });
        tx.send(task).unwrap();
    }
    drop(tx);
    while let Ok(task) = rx.recv() {
        task.queued.store(false, Ordering::Release);
        let waker = Waker::from(task.clone());
        let mut slot = task.future.lock().unwrap();
        if let Some(f) = slot.as_mut() {
            if f.as_mut().poll(&mut Context::from_waker(&waker)).is_ready() {
                *slot = None;
            }
        }
    }
}

fn run_local_pool(n: u64) {
    use futures::task::LocalSpawnExt;
    let mut pool = futures::executor::LocalPool::new();
    let spawner = pool.spawner();
    for id in 0..n {
        spawner.spawn_local(async move { std::hint::black_box(handler(id).await); }).unwrap();
    }
    pool.run();
}

fn measure(label: &str, n: u64, f: impl FnOnce(u64)) {
    let (a0, b0) = (ALLOCS.load(Ordering::Relaxed), BYTES.load(Ordering::Relaxed));
    let start = Instant::now();
    f(n);
    let ns = start.elapsed().as_nanos() as f64 / n as f64;
    let allocs = (ALLOCS.load(Ordering::Relaxed) - a0) as f64 / n as f64;
    let bytes = (BYTES.load(Ordering::Relaxed) - b0) as f64 / n as f64;
    println!("{label:<34} {allocs:>5.2} allocs, {bytes:>6.1} bytes, {ns:>9.0} ns per unit");
}

fn main() {
    println!("handler future: {} bytes", size_of_val(&handler(1)));
    measure("our executor, 100,000 tasks:", 100_000, run_ours);
    measure("futures LocalPool, 100,000 tasks:", 100_000, run_local_pool);
    measure("OS threads, 1,000 spawn + join:", 1_000, |n| {
        for id in 0..n {
            std::thread::spawn(move || std::hint::black_box(id * 2)).join().unwrap();
        }
    });
}
