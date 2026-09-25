// verify: debug ok
// ABA, replayed deterministically. A lock-free free list of slot indices (a Treiber stack over indices,
// as in an object pool). Thread A's pop is split into its two steps so we can "preempt" it between them.
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::{AcqRel, Acquire, Relaxed, Release}};

const NIL: u32 = u32::MAX;

/// Naive: the head is just an index.
pub struct FreeList {
    head: AtomicU32,
    next: Vec<AtomicU32>,
}

impl FreeList {
    pub fn with_slots(n: u32) -> Self {
        let next = (0..n).map(|i| AtomicU32::new(if i + 1 < n { i + 1 } else { NIL })).collect();
        FreeList { head: AtomicU32::new(0), next } // 0 → 1 → … → n-1
    }
    /// Pop, step 1: read the head and its successor.
    pub fn pop_begin(&self) -> Option<(u32, u32)> {
        let h = self.head.load(Acquire);
        (h != NIL).then(|| (h, self.next[h as usize].load(Relaxed)))
    }
    /// Pop, step 2: swing head from `h` to `next` if head is still `h`.
    pub fn pop_commit(&self, h: u32, next: u32) -> bool {
        self.head.compare_exchange(h, next, AcqRel, Acquire).is_ok()
    }
    pub fn pop(&self) -> Option<u32> {
        loop {
            let (h, n) = self.pop_begin()?;
            if self.pop_commit(h, n) {
                return Some(h);
            }
        }
    }
    pub fn push(&self, slot: u32) {
        let mut h = self.head.load(Relaxed);
        loop {
            self.next[slot as usize].store(h, Relaxed);
            match self.head.compare_exchange(h, slot, Release, Relaxed) {
                Ok(_) => return,
                Err(cur) => h = cur,
            }
        }
    }
}

/// Fixed: the head carries a version tag that every successful CAS increments: (tag << 32) | index.
pub struct TaggedFreeList {
    head: AtomicU64,
    next: Vec<AtomicU32>,
}

fn pack(tag: u32, idx: u32) -> u64 { ((tag as u64) << 32) | idx as u64 }
fn unpack(w: u64) -> (u32, u32) { ((w >> 32) as u32, w as u32) }

impl TaggedFreeList {
    pub fn with_slots(n: u32) -> Self {
        let next = (0..n).map(|i| AtomicU32::new(if i + 1 < n { i + 1 } else { NIL })).collect();
        TaggedFreeList { head: AtomicU64::new(pack(0, 0)), next }
    }
    pub fn pop_begin(&self) -> Option<(u64, u32)> {
        let w = self.head.load(Acquire);
        let (_, h) = unpack(w);
        (h != NIL).then(|| (w, self.next[h as usize].load(Relaxed)))
    }
    pub fn pop_commit(&self, w: u64, next: u32) -> bool {
        let (tag, _) = unpack(w);
        self.head.compare_exchange(w, pack(tag.wrapping_add(1), next), AcqRel, Acquire).is_ok()
    }
    pub fn pop(&self) -> Option<u32> {
        loop {
            let (w, n) = self.pop_begin()?;
            if self.pop_commit(w, n) {
                return Some(unpack(w).1);
            }
        }
    }
    pub fn push(&self, slot: u32) {
        let mut w = self.head.load(Relaxed);
        loop {
            let (tag, h) = unpack(w);
            self.next[slot as usize].store(h, Relaxed);
            match self.head.compare_exchange(w, pack(tag.wrapping_add(1), slot), Release, Relaxed) {
                Ok(_) => return,
                Err(cur) => w = cur,
            }
        }
    }
}

fn main() {
    // Free list: 0 → 1 → 2.
    let fl = FreeList::with_slots(3);
    let (h, next) = fl.pop_begin().unwrap(); // A: reads head = 0, next = 1 … and is preempted
    let b1 = fl.pop().unwrap(); //              B: takes slot 0
    let b2 = fl.pop().unwrap(); //              B: takes slot 1
    fl.push(b1); //                             B: returns slot 0 → list is 0 → 2
    let ok = fl.pop_commit(h, next); //         A: head is 0 again, so its CAS succeeds: head = 1 (!)
    let c = fl.pop().unwrap(); //               C: takes slot 1, which B still owns
    println!("naive:  A's stale CAS succeeded = {ok}; A got slot {h}; C got slot {c}; B still owns slot {b2}");

    let tl = TaggedFreeList::with_slots(3);
    let (w, next) = tl.pop_begin().unwrap(); // A: reads (tag 0, head 0), next = 1 … preempted
    let b1 = tl.pop().unwrap();
    let b2 = tl.pop().unwrap();
    tl.push(b1); //                             head is index 0 again, but with tag 3
    let ok = tl.pop_commit(w, next); //         A: tag 0 ≠ 3, so the CAS fails
    let a = tl.pop().unwrap(); //               A retries from scratch
    let c = tl.pop().unwrap();
    println!("tagged: A's stale CAS succeeded = {ok}; A retried and got slot {a}; C got slot {c}; B owns slot {b2}");
}
