// verify: debug ok
// verify: debug miri-ok
// MyVec<T>, step 4: Drain with LEAK AMPLIFICATION. `drain` shrinks the vector's len to `start`
// BEFORE handing out anything, so forgetting the Drain (mem::forget is safe) can only leak.
// Core identical to ch04-01-myvec-core.rs.
use std::alloc::{self, Layout};
use std::marker::PhantomData;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::ptr::{self, NonNull};

/// Owns room for `cap` values of `T`. Knows nothing about which slots hold values.
struct RawVec<T> {
    ptr: NonNull<T>,
    /// INVARIANT: T is a ZST => cap == usize::MAX and nothing is allocated.
    /// Otherwise cap == 0 (ptr is dangling, nothing allocated)
    /// or ptr came from the global allocator with Layout::array::<T>(cap).
    cap: usize,
    _owns: PhantomData<T>,
}

// SAFETY: a RawVec<T> uniquely owns its buffer (like Box<[T]>), so sending it sends the T's.
unsafe impl<T: Send> Send for RawVec<T> {}
// SAFETY: shared access to a RawVec<T> only ever hands out shared access to T's.
unsafe impl<T: Sync> Sync for RawVec<T> {}

impl<T> RawVec<T> {
    const IS_ZST: bool = mem::size_of::<T>() == 0;

    fn new() -> Self {
        let cap = if Self::IS_ZST { usize::MAX } else { 0 };
        RawVec { ptr: NonNull::dangling(), cap, _owns: PhantomData }
    }

    #[cold]
    fn grow(&mut self) {
        // A ZST RawVec already has cap == usize::MAX: getting here means the length overflowed.
        assert!(!Self::IS_ZST, "capacity overflow");
        let new_cap = if self.cap == 0 { 4 } else { self.cap.checked_mul(2).expect("capacity overflow") };
        // Layout::array also enforces the language rule: an allocation is at most isize::MAX bytes.
        let new_layout = Layout::array::<T>(new_cap).expect("capacity overflow");
        let new_ptr = if self.cap == 0 {
            // SAFETY: new_layout has a non-zero size (T is not a ZST and new_cap > 0).
            unsafe { alloc::alloc(new_layout) }
        } else {
            let old_layout = Layout::array::<T>(self.cap).unwrap();
            // SAFETY: ptr was allocated with old_layout (invariant); the new size is non-zero
            // and was validated by Layout::array.
            unsafe { alloc::realloc(self.ptr.as_ptr().cast::<u8>(), old_layout, new_layout.size()) }
        };
        self.ptr = match NonNull::new(new_ptr.cast::<T>()) {
            Some(p) => p,
            None => alloc::handle_alloc_error(new_layout), // out of memory: abort, like std's Vec
        };
        self.cap = new_cap;
    }
}

impl<T> Drop for RawVec<T> {
    fn drop(&mut self) {
        if self.cap != 0 && !Self::IS_ZST {
            // SAFETY: invariant: ptr was allocated with exactly this layout. Only memory is freed;
            // whoever owned values in it (MyVec, IntoIter, ...) must have dropped them already.
            unsafe { alloc::dealloc(self.ptr.as_ptr().cast::<u8>(), Layout::array::<T>(self.cap).unwrap()) }
        }
    }
}

pub struct MyVec<T> {
    buf: RawVec<T>,
    /// INVARIANT: len <= buf.cap, and exactly the slots [0, len) are initialized.
    len: usize,
}

impl<T> MyVec<T> {
    pub fn new() -> Self {
        MyVec { buf: RawVec::new(), len: 0 }
    }

    pub fn capacity(&self) -> usize {
        self.buf.cap
    }

    fn ptr(&self) -> *mut T {
        self.buf.ptr.as_ptr()
    }

    pub fn push(&mut self, value: T) {
        if self.len == self.buf.cap {
            self.buf.grow();
        }
        // SAFETY: len < cap, so slot `len` is inside the buffer (or T is a ZST) and uninitialized.
        unsafe { self.ptr().add(self.len).write(value) };
        self.len += 1; // publish the slot only after it holds a value
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1; // unpublish first: the slot is now outside [0, len)
        // SAFETY: slot `len` was initialized and is now outside [0, len): moved out exactly once.
        Some(unsafe { self.ptr().add(self.len).read() })
    }
}

impl<T> Drop for MyVec<T> {
    fn drop(&mut self) {
        // SAFETY: [0, len) are initialized; drop them in place. RawVec's Drop then frees the memory.
        unsafe { ptr::drop_in_place(ptr::slice_from_raw_parts_mut(self.ptr(), self.len)) }
    }
}

impl<T> Deref for MyVec<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        // SAFETY: ptr is non-null and aligned (dangling is allowed for len 0 and for ZSTs),
        // [0, len) are initialized, and the borrow of self keeps the buffer alive and unmodified.
        unsafe { std::slice::from_raw_parts(self.ptr(), self.len) }
    }
}

impl<T> DerefMut for MyVec<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        // SAFETY: as in deref; &mut self makes the access exclusive.
        unsafe { std::slice::from_raw_parts_mut(self.ptr(), self.len) }
    }
}

use std::ops::Range;

pub struct Drain<'a, T> {
    vec: &'a mut MyVec<T>,
    /// INVARIANT: [next, end) are initialized and owned by the Drain (they are outside vec[0, len)).
    next: usize,
    end: usize,
    /// The elements after the drained range: [tail_start, tail_start + tail_len), to move back on drop.
    tail_start: usize,
    tail_len: usize,
}

impl<T> MyVec<T> {
    pub fn drain(&mut self, range: Range<usize>) -> Drain<'_, T> {
        let Range { start, end } = range;
        assert!(start <= end && end <= self.len, "drain range out of bounds");
        let tail_len = self.len - end;
        // LEAK AMPLIFICATION: from here on the vector claims only [0, start). If the Drain is
        // forgotten, the drained range and the tail leak, but nothing is dropped twice.
        self.len = start;
        Drain { vec: self, next: start, end, tail_start: end, tail_len }
    }
}

impl<T> Iterator for Drain<'_, T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        if self.next == self.end {
            return None;
        }
        let i = self.next;
        self.next += 1;
        // SAFETY: slot i was in [next, end): initialized, owned by us, and read exactly once.
        Some(unsafe { self.vec.ptr().add(i).read() })
    }
}

impl<T> Drop for Drain<'_, T> {
    fn drop(&mut self) {
        // 1. Drop the drained elements the caller didn't take. (std also guards this step against
        //    a panicking destructor so the tail is still moved back; see the exercises.)
        for x in self.by_ref() {
            drop(x);
        }
        // 2. Slide the tail down to `start` and give it back to the vector.
        let start = self.vec.len;
        // SAFETY: the tail is initialized and still in place; [start, start + tail_len) is inside the
        // buffer; ptr::copy allows overlap. Afterwards [0, start + tail_len) is initialized.
        unsafe {
            let base = self.vec.ptr();
            ptr::copy(base.add(self.tail_start), base.add(start), self.tail_len);
        }
        self.vec.len = start + self.tail_len;
    }
}

use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
static DROPPED_IDS: AtomicU32 = AtomicU32::new(0); // sum of the ids of dropped Tracked values

struct Tracked(u32); // no heap memory, so leaking one is harmless (and Miri has no leak to report)
impl Drop for Tracked {
    fn drop(&mut self) {
        DROPPED_IDS.fetch_add(self.0, Relaxed);
    }
}

fn main() {
    // Normal use: drain a range, keep the rest.
    let mut v: MyVec<String> = MyVec::new();
    for s in ["a", "b", "c", "d", "e", "f"] {
        v.push(s.to_string());
    }
    let taken: Vec<String> = v.drain(1..4).collect();
    println!("taken={taken:?} left={:?}", &v[..]);

    // Partial consumption: the Drain drops what the caller didn't take, then restores the tail.
    let mut d = v.drain(0..2);
    let first = d.next();
    drop(d);
    println!("first={first:?} left={:?}", &v[..]);

    // mem::forget is SAFE, so a Drain may never run its destructor. Result: a leak, not UB.
    let mut w: MyVec<Tracked> = MyVec::new();
    for id in [1, 10, 100, 1000, 10000] {
        w.push(Tracked(id));
    }
    let mut d = w.drain(1..3);
    let ten = d.next();
    std::mem::forget(d);
    println!("after forget: w.len()={} (the drained range and the tail are leaked)", w.len());
    drop(ten);
    drop(w);
    println!("sum of dropped ids = {} (1 + 10; 100, 1000, 10000 were leaked)", DROPPED_IDS.load(Relaxed));
}
