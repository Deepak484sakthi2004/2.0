// verify: release ok
// Chapter 9.4's promise: benchmark the order-book redesign. The same deterministic stream of 1,000,000 operations
// (60% limit orders, 30% cancels, 10% market orders) against two books with the same logic (FIFO levels, lazy cancel):
//   Java-shaped (the Part III review's design): every order an Rc<RefCell<Order>>, levels hold Rc clones
//   arena (Chapter 9.4's redesign): orders in a Slab, levels hold slab indices
// ns per operation (p50/p99/p99.9 from per-batch timing) and heap allocations per operation. One Playground run, noisy.
use hdrhistogram::Histogram;
use slab::Slab;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::hint::black_box;
use std::rc::Rc;
use std::time::Instant;

mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    pub static ALLOCS: AtomicUsize = AtomicUsize::new(0);
    pub struct Counting;
    // SAFETY: both methods forward their exact arguments to `System`, which upholds the
    // GlobalAlloc contract; the counter is a plain atomic, so counting never allocates.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }
    #[global_allocator]
    static GLOBAL: Counting = Counting;
}

#[derive(Clone, Copy)]
enum Op {
    Limit { id: u64, bid: bool, price: i64, qty: u32 },
    Cancel { id: u64 },
    Market { buy: bool, qty: u32 },
}

struct Order {
    id: u64,
    qty: u32,
    live: bool,
}

trait Book {
    fn limit(&mut self, id: u64, bid: bool, price: i64, qty: u32);
    fn cancel(&mut self, id: u64);
    /// Returns filled quantity.
    fn market(&mut self, buy: bool, qty: u32) -> u32;
}

// ---------- Java-shaped: shared, mutable, reference-counted orders ----------
#[derive(Default)]
struct RcBook {
    bids: BTreeMap<i64, VecDeque<Rc<RefCell<Order>>>>,
    asks: BTreeMap<i64, VecDeque<Rc<RefCell<Order>>>>,
    by_id: HashMap<u64, Rc<RefCell<Order>>>,
}

impl Book for RcBook {
    fn limit(&mut self, id: u64, bid: bool, price: i64, qty: u32) {
        let o = Rc::new(RefCell::new(Order { id, qty, live: true }));
        let side = if bid { &mut self.bids } else { &mut self.asks };
        side.entry(price).or_default().push_back(o.clone());
        self.by_id.insert(id, o);
    }
    fn cancel(&mut self, id: u64) {
        if let Some(o) = self.by_id.remove(&id) {
            o.borrow_mut().live = false; // lazy: removed from its level when matching reaches it
        }
    }
    fn market(&mut self, buy: bool, mut qty: u32) -> u32 {
        let side = if buy { &mut self.asks } else { &mut self.bids };
        let mut filled = 0;
        while qty > 0 {
            let Some(mut level) = (if buy { side.first_entry() } else { side.last_entry() }) else { break };
            let q = level.get_mut();
            while qty > 0 {
                let Some(front) = q.front() else { break };
                let mut o = front.borrow_mut();
                if !o.live {
                    drop(o);
                    q.pop_front();
                    continue;
                }
                let take = o.qty.min(qty);
                o.qty -= take;
                qty -= take;
                filled += take;
                if o.qty == 0 {
                    let id = o.id;
                    drop(o);
                    q.pop_front();
                    self.by_id.remove(&id);
                }
            }
            if q.is_empty() {
                level.remove();
            }
        }
        filled
    }
}

// ---------- Arena: orders in a Slab, levels hold indices ----------
#[derive(Default)]
struct ArenaBook {
    orders: Slab<Order>,
    bids: BTreeMap<i64, VecDeque<usize>>,
    asks: BTreeMap<i64, VecDeque<usize>>,
    by_id: HashMap<u64, usize>,
}

impl Book for ArenaBook {
    fn limit(&mut self, id: u64, bid: bool, price: i64, qty: u32) {
        let k = self.orders.insert(Order { id, qty, live: true });
        let side = if bid { &mut self.bids } else { &mut self.asks };
        side.entry(price).or_default().push_back(k);
        self.by_id.insert(id, k);
    }
    fn cancel(&mut self, id: u64) {
        if let Some(k) = self.by_id.remove(&id) {
            self.orders[k].live = false;
        }
    }
    fn market(&mut self, buy: bool, mut qty: u32) -> u32 {
        let side = if buy { &mut self.asks } else { &mut self.bids };
        let mut filled = 0;
        while qty > 0 {
            let Some(mut level) = (if buy { side.first_entry() } else { side.last_entry() }) else { break };
            let q = level.get_mut();
            while qty > 0 {
                let Some(&k) = q.front() else { break };
                let o = &mut self.orders[k];
                if !o.live {
                    q.pop_front();
                    self.orders.remove(k); // the slot is reused by the next insert
                    continue;
                }
                let take = o.qty.min(qty);
                o.qty -= take;
                qty -= take;
                filled += take;
                if o.qty == 0 {
                    let id = o.id;
                    q.pop_front();
                    self.orders.remove(k);
                    self.by_id.remove(&id);
                }
            }
            if q.is_empty() {
                level.remove();
            }
        }
        filled
    }
}

fn stream(n: usize) -> Vec<Op> {
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let mut live: Vec<u64> = Vec::new();
    let mut id = 0u64;
    (0..n)
        .map(|_| {
            let r = next() % 100;
            if r < 60 || live.is_empty() {
                id += 1;
                live.push(id);
                let bid = next() % 2 == 0;
                let off = (next() % 50) as i64 + 1; // 1..50 ticks away from a mid of 10,000
                Op::Limit { id, bid, price: if bid { 10_000 - off } else { 10_000 + off }, qty: 1 + (next() % 100) as u32 }
            } else if r < 90 {
                let i = (next() as usize) % live.len();
                Op::Cancel { id: live.swap_remove(i) }
            } else {
                Op::Market { buy: next() % 2 == 0, qty: 1 + (next() % 300) as u32 }
            }
        })
        .collect()
}

fn run(name: &str, book: &mut dyn Book, ops: &[Op]) {
    let mut h = Histogram::<u64>::new_with_bounds(1, 100_000_000, 3).unwrap();
    let a0 = counting::ALLOCS.load(std::sync::atomic::Ordering::Relaxed);
    let t = Instant::now();
    let mut filled = 0u64;
    for batch in ops.chunks(64) {
        let t0 = Instant::now();
        for op in batch {
            match *op {
                Op::Limit { id, bid, price, qty } => book.limit(id, bid, price, qty),
                Op::Cancel { id } => book.cancel(id),
                Op::Market { buy, qty } => filled += book.market(buy, qty) as u64,
            }
        }
        h.record(t0.elapsed().as_nanos() as u64 / batch.len() as u64).unwrap();
    }
    let total = t.elapsed().as_secs_f64() * 1e3;
    let allocs = counting::ALLOCS.load(std::sync::atomic::Ordering::Relaxed) - a0;
    black_box(filled);
    println!(
        "{name:<14} total {total:6.1} ms   ns/op p50 {:>4} p99 {:>5} p99.9 {:>6}   allocs/op {:.2}   filled {filled}",
        h.value_at_percentile(50.0),
        h.value_at_percentile(99.0),
        h.value_at_percentile(99.9),
        allocs as f64 / ops.len() as f64
    );
}

fn main() {
    let ops = stream(1_000_000);
    run("Java-shaped", &mut RcBook::default(), &ops);
    run("arena", &mut ArenaBook::default(), &ops);
}
