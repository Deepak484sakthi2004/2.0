// verify: debug ok
// verify: debug miri-ok
// MyVec<T>, step 3: IntoIter takes over the buffer AND the elements; it must drop whatever
// the caller doesn't consume. Core identical to ch04-01-myvec-core.rs.
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


pub struct IntoIter<T> {
    buf: RawVec<T>, // frees the memory when the iterator is dropped
    /// INVARIANT: next <= end <= buf.cap, and exactly the slots [next, end) are initialized.
    next: usize,
    end: usize,
}

impl<T> IntoIterator for MyVec<T> {
    type Item = T;
    type IntoIter = IntoIter<T>;
    fn into_iter(self) -> IntoIter<T> {
        // Don't run MyVec::drop: the elements now belong to the iterator.
        let me = mem::ManuallyDrop::new(self);
        // SAFETY: `buf` is read out exactly once and `me` is never used or dropped afterwards,
        // so the buffer has exactly one owner again.
        let buf = unsafe { ptr::read(&me.buf) };
        IntoIter { buf, next: 0, end: me.len }
    }
}

impl<T> Iterator for IntoIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        if self.next == self.end {
            return None;
        }
        let i = self.next;
        self.next += 1; // slot i leaves [next, end) before we move out of it
        // SAFETY: slot i was in [next, end), so it is initialized; it is read exactly once.
        Some(unsafe { self.buf.ptr.as_ptr().add(i).read() })
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.end - self.next;
        (n, Some(n))
    }
}

impl<T> DoubleEndedIterator for IntoIter<T> {
    fn next_back(&mut self) -> Option<T> {
        if self.next == self.end {
            return None;
        }
        self.end -= 1;
        // SAFETY: slot `end` was the last initialized slot and is now outside [next, end).
        Some(unsafe { self.buf.ptr.as_ptr().add(self.end).read() })
    }
}

impl<T> Drop for IntoIter<T> {
    fn drop(&mut self) {
        // SAFETY: [next, end) are initialized and owned by us; drop them, then RawVec frees the buffer.
        unsafe {
            let first = self.buf.ptr.as_ptr().add(self.next);
            ptr::drop_in_place(ptr::slice_from_raw_parts_mut(first, self.end - self.next));
        }
    }
}

use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
static DROPS: AtomicUsize = AtomicUsize::new(0);

struct Ticket(String);
impl Drop for Ticket {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Relaxed);
    }
}

fn main() {
    let mut v = MyVec::new();
    for id in 1..=6 {
        v.push(Ticket(format!("T{id}")));
    }
    let mut it = v.into_iter();
    let first = it.next();
    let last = it.next_back();
    let name = |t: &Option<Ticket>| t.as_ref().map(|t| t.0.clone());
    println!("first={:?} last={:?} remaining={}", name(&first), name(&last), it.size_hint().0);
    drop(it); // the four tickets nobody consumed are dropped here, once each
    println!("drops after dropping the iterator: {}", DROPS.load(Relaxed));
    drop((first, last));
    println!("drops in total: {}", DROPS.load(Relaxed));
}
