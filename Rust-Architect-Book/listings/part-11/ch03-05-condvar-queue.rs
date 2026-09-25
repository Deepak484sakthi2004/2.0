// verify: debug ok
// verify: release ok
//! A bounded blocking queue from a Mutex and two Condvars: the classic monitor, Rust-style.
//! Producers block when it's full (backpressure), consumers block when it's empty, close() ends it.
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
use std::thread;

pub struct BoundedQueue<T> {
    state: Mutex<State<T>>,
    not_empty: Condvar,
    not_full: Condvar,
    capacity: usize,
    pub producer_waits: AtomicU64, // how often a producer found the queue full
}

struct State<T> {
    items: VecDeque<T>,
    closed: bool,
}

#[derive(Debug, PartialEq)]
pub enum PushError<T> {
    Full(T),   // try_push only: the queue is at capacity
    Closed(T), // nobody will ever pop it
}

impl<T> BoundedQueue<T> {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0);
        BoundedQueue {
            state: Mutex::new(State { items: VecDeque::with_capacity(capacity), closed: false }),
            not_empty: Condvar::new(),
            not_full: Condvar::new(),
            capacity,
            producer_waits: AtomicU64::new(0),
        }
    }

    /// Blocks while the queue is full.
    pub fn push(&self, item: T) -> Result<(), PushError<T>> {
        let mut s = self.state.lock().unwrap();
        if s.items.len() == self.capacity && !s.closed {
            self.producer_waits.fetch_add(1, Ordering::Relaxed);
        }
        // wait_while re-checks the condition after every wakeup, spurious ones included.
        s = self.not_full.wait_while(s, |s| s.items.len() == self.capacity && !s.closed).unwrap();
        if s.closed {
            return Err(PushError::Closed(item));
        }
        s.items.push_back(item);
        drop(s); // unlock before notifying: the woken consumer can take the lock immediately
        self.not_empty.notify_one();
        Ok(())
    }

    /// Never blocks: hands the item back if the queue is full (load shedding).
    pub fn try_push(&self, item: T) -> Result<(), PushError<T>> {
        let mut s = self.state.lock().unwrap();
        if s.closed {
            return Err(PushError::Closed(item));
        }
        if s.items.len() == self.capacity {
            return Err(PushError::Full(item));
        }
        s.items.push_back(item);
        drop(s);
        self.not_empty.notify_one();
        Ok(())
    }

    /// Blocks while empty; None once the queue is closed AND drained.
    pub fn pop(&self) -> Option<T> {
        let mut s = self.state.lock().unwrap();
        s = self.not_empty.wait_while(s, |s| s.items.is_empty() && !s.closed).unwrap();
        let item = s.items.pop_front();
        drop(s);
        if item.is_some() {
            self.not_full.notify_one();
        }
        item
    }

    pub fn close(&self) {
        self.state.lock().unwrap().closed = true;
        self.not_empty.notify_all();
        self.not_full.notify_all();
    }
}

fn main() {
    let q = BoundedQueue::new(8);
    let consumed = AtomicU64::new(0);
    let sum = AtomicU64::new(0);

    thread::scope(|s| {
        for _ in 0..3 {
            s.spawn(|| {
                while let Some(v) = q.pop() {
                    sum.fetch_add(v, Ordering::Relaxed);
                    consumed.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
        let producers: Vec<_> = (0..2u64)
            .map(|p| {
                let q = &q;
                s.spawn(move || {
                    for i in 0..10_000 {
                        q.push(p * 10_000 + i).unwrap();
                    }
                })
            })
            .collect();
        for p in producers {
            p.join().unwrap();
        }
        q.close(); // consumers drain what's left, then pop() returns None
    });

    let expected: u64 = (0..20_000).sum();
    println!("consumed {} items, sum ok = {}", consumed.into_inner(), sum.into_inner() == expected);
    println!("a producer ever found the queue full (backpressure): {}", q.producer_waits.load(Ordering::Relaxed) > 0);

    let small = BoundedQueue::new(1);
    println!("try_push into empty: {:?}", small.try_push("a"));
    println!("try_push into full:  {:?}", small.try_push("b"));
    small.close();
    println!("push after close:    {:?}", small.push("c"));
    println!("pop after close:     {:?}, then {:?}", small.pop(), small.pop());
}
