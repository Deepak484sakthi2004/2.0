// verify: release ok
//! Chapter 12.5 production scenario: a deterministic simulation executor.
//! Virtual time (sleeps complete instantly, in deadline order) and a seeded scheduler that picks a
//! random ready task. Same seed, same interleaving, every time: a race found once can be replayed.
//! Under test: payments-core's idempotency check, with and without an await between check and act.
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

struct Sim {
    now_ms: Cell<u64>,
    rng: Cell<u64>,
    timers: RefCell<BTreeMap<(u64, u64), Waker>>, // (deadline, seq) -> waker
    seq: Cell<u64>,
    trace: RefCell<Vec<String>>,
}

impl Sim {
    fn new(seed: u64) -> Rc<Sim> {
        Rc::new(Sim {
            now_ms: Cell::new(0),
            rng: Cell::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1),
            timers: RefCell::default(),
            seq: Cell::new(0),
            trace: RefCell::default(),
        })
    }
    /// xorshift64: tiny, deterministic, good enough to pick schedules.
    fn rand(&self, n: u64) -> u64 {
        let mut x = self.rng.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng.set(x);
        x % n
    }
    fn log(&self, s: String) {
        self.trace.borrow_mut().push(format!("t={}ms {s}", self.now_ms.get()));
    }
    fn sleep(self: &Rc<Self>, ms: u64) -> SimSleep {
        SimSleep { sim: self.clone(), deadline: self.now_ms.get() + ms, key: None }
    }
}

struct SimSleep {
    sim: Rc<Sim>,
    deadline: u64,
    key: Option<(u64, u64)>,
}
impl Future for SimSleep {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.sim.now_ms.get() >= self.deadline {
            return Poll::Ready(());
        }
        let key = match self.key {
            Some(k) => k,
            None => {
                let s = self.sim.seq.get() + 1;
                self.sim.seq.set(s);
                self.key = Some((self.deadline, s));
                (self.deadline, s)
            }
        };
        self.sim.timers.borrow_mut().insert(key, cx.waker().clone()); // refreshes the waker too
        Poll::Pending
    }
}

struct IdWaker(usize, Arc<Mutex<Vec<usize>>>);
impl Wake for IdWaker {
    fn wake(self: Arc<Self>) {
        let mut ready = self.1.lock().unwrap();
        if !ready.contains(&self.0) {
            ready.push(self.0);
        }
    }
}

/// Runs all tasks to completion under virtual time with a seeded, random choice of ready task.
fn run(sim: &Rc<Sim>, tasks: Vec<Pin<Box<dyn Future<Output = ()>>>>) {
    let ready = Arc::new(Mutex::new((0..tasks.len()).collect::<Vec<_>>()));
    let mut tasks: Vec<Option<_>> = tasks.into_iter().map(Some).collect();
    loop {
        let pick = {
            let mut r = ready.lock().unwrap();
            if r.is_empty() {
                None
            } else {
                let i = sim.rand(r.len() as u64) as usize;
                Some(r.swap_remove(i))
            }
        };
        match pick {
            Some(id) => {
                let waker = Waker::from(Arc::new(IdWaker(id, ready.clone())));
                if let Some(f) = tasks[id].as_mut() {
                    if f.as_mut().poll(&mut Context::from_waker(&waker)).is_ready() {
                        tasks[id] = None;
                    }
                }
            }
            None => {
                // Nothing runnable: jump the clock to the next timer. No real waiting.
                let Some(((deadline, _), w)) = sim.timers.borrow_mut().pop_first() else { return };
                sim.now_ms.set(deadline);
                w.wake();
            }
        }
    }
}

type Store = Rc<RefCell<HashMap<&'static str, String>>>;

/// v1: check, then await the fraud score, then charge and record. The bug: an .await between check and act.
async fn charge_v1(sim: Rc<Sim>, store: Store, calls: Rc<Cell<u32>>, key: &'static str, req: u32) {
    if let Some(prev) = store.borrow().get(key) {
        sim.log(format!("req {req}: duplicate, returns {prev}"));
        return;
    }
    sim.log(format!("req {req}: key not found, scoring"));
    sim.sleep(1 + sim.rand(5)).await; // fraud score lookup
    calls.set(calls.get() + 1);
    let id = format!("ch_{}", calls.get());
    sim.log(format!("req {req}: charged {id}"));
    store.borrow_mut().insert(key, id);
}

/// v2: reserve the key before any await: check-and-reserve is one synchronous step.
async fn charge_v2(sim: Rc<Sim>, store: Store, calls: Rc<Cell<u32>>, key: &'static str, req: u32) {
    if let Some(prev) = store.borrow().get(key) {
        sim.log(format!("req {req}: duplicate, returns {prev}"));
        return;
    }
    store.borrow_mut().insert(key, "in-progress".to_string());
    sim.log(format!("req {req}: key reserved, scoring"));
    sim.sleep(1 + sim.rand(5)).await;
    calls.set(calls.get() + 1);
    let id = format!("ch_{}", calls.get());
    sim.log(format!("req {req}: charged {id}"));
    store.borrow_mut().insert(key, id);
}

/// One simulated scenario: a client retry sends the same idempotency key twice, 0-19 ms apart.
fn scenario(seed: u64, fixed: bool) -> (u32, Vec<String>) {
    let sim = Sim::new(seed);
    let (store, calls): (Store, _) = (Rc::default(), Rc::new(Cell::new(0)));
    let mut tasks: Vec<Pin<Box<dyn Future<Output = ()>>>> = Vec::new();
    for req in 1..=2 {
        let (sim2, store, calls) = (sim.clone(), store.clone(), calls.clone());
        let gap = if req == 1 { 0 } else { sim.rand(20) };
        tasks.push(Box::pin(async move {
            sim2.sleep(gap).await; // the retry arrives a little later
            if fixed {
                charge_v2(sim2, store, calls, "idem-42", req).await
            } else {
                charge_v1(sim2, store, calls, "idem-42", req).await
            }
        }));
    }
    run(&sim, tasks);
    let trace = sim.trace.take();
    (calls.get(), trace)
}

fn main() {
    for fixed in [false, true] {
        let failing: Vec<u64> = (1..=1_000).filter(|&seed| scenario(seed, fixed).0 > 1).collect();
        println!("{}: {} of 1000 seeds double-charge", if fixed { "v2 (reserve first)" } else { "v1 (check, await, act)" }, failing.len());
        if let Some(&seed) = failing.first() {
            let (a, trace) = scenario(seed, fixed);
            let (b, again) = scenario(seed, fixed);
            println!("  first failing seed {seed}: {a} charges; replay identical: {}", a == b && trace == again);
            for line in trace {
                println!("    {line}");
            }
        }
    }
}
