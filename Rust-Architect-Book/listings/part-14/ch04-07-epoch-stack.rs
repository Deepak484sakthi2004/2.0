// verify: release ok
// verify: debug+tree miri-ok
// (Tree Borrows: under Stacked Borrows, Miri flags crossbeam-epoch 0.9.20's own intrusive list, a
// container_of-style cast in `Local::element_of`, which Tree Borrows accepts. See Chapter 15.2.)
//
// A Treiber stack whose popped nodes are freed only when no thread can still be reading them:
// epoch-based reclamation with crossbeam_epoch (after the crate's own Treiber-stack example).
// The stack owns its Collector, so dropping the stack runs every deferred free.
use crossbeam_epoch::{Atomic, Collector, LocalHandle, Owned};
use std::mem::ManuallyDrop;
use std::ptr;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::thread;

pub struct TreiberStack<T> {
    head: Atomic<Node<T>>,
    collector: Collector,
}

struct Node<T> {
    data: ManuallyDrop<T>,
    next: Atomic<Node<T>>,
}

impl<T> TreiberStack<T> {
    pub fn new() -> Self {
        TreiberStack { head: Atomic::null(), collector: Collector::new() }
    }

    /// Each thread registers once and passes its handle to every operation.
    pub fn handle(&self) -> LocalHandle {
        self.collector.register()
    }

    pub fn push(&self, t: T, h: &LocalHandle) {
        let mut n = Owned::new(Node { data: ManuallyDrop::new(t), next: Atomic::null() });
        let guard = h.pin();
        loop {
            let head = self.head.load(Relaxed, &guard);
            n.next.store(head, Relaxed);
            // Release: the node's contents are published together with the pointer to it.
            match self.head.compare_exchange(head, n, Release, Relaxed, &guard) {
                Ok(_) => break,
                Err(e) => n = e.new, // lost the race: take our node back and retry
            }
        }
    }

    pub fn pop(&self, h: &LocalHandle) -> Option<T> {
        let guard = h.pin(); // while pinned, no node we can see will be freed under us
        loop {
            let head = self.head.load(Acquire, &guard); // Acquire: pairs with push's Release
            let node = unsafe { head.as_ref() }?; // SAFETY: we're pinned, so `head` isn't reclaimed yet
            let next = node.next.load(Relaxed, &guard);
            if self.head.compare_exchange(head, next, Relaxed, Relaxed, &guard).is_ok() {
                unsafe {
                    // SAFETY: we unlinked `head`, but other pinned threads may still be reading it: defer
                    // the free until they've all unpinned. The data is moved out exactly once, by us.
                    guard.defer_destroy(head);
                    return Some(ManuallyDrop::into_inner(ptr::read(&node.data)));
                }
            }
        }
    }
}

impl<T> Drop for TreiberStack<T> {
    fn drop(&mut self) {
        let h = self.handle();
        while self.pop(&h).is_some() {}
    } // then `collector` drops: its remaining garbage (deferred frees) is released here
}

const THREADS: u64 = 4;
const PER_THREAD: u64 = if cfg!(miri) { 20 } else { 200_000 };

fn main() {
    let stack = TreiberStack::new();
    let popped: u64 = thread::scope(|s| {
        let hs: Vec<_> = (0..THREADS)
            .map(|t| {
                let stack = &stack;
                s.spawn(move || {
                    let h = stack.handle();
                    let mut sum = 0;
                    for i in 0..PER_THREAD {
                        stack.push(t * PER_THREAD + i, &h);
                        sum += stack.pop(&h).unwrap_or(0); // pops something (not necessarily ours)
                    }
                    sum
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).sum()
    });
    let n = THREADS * PER_THREAD;
    println!("pushed {n} values, popped sum = {popped} (expected {})", n * (n - 1) / 2);
}
