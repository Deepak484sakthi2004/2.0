// verify: debug miri use-after-free
// BROKEN on purpose: a Drain WITHOUT leak amplification. It leaves the vector's len alone and fixes
// it up in Drop. Safe code can skip that Drop with mem::forget: the vector then still owns a String
// that the caller already received and dropped. Core identical to ch04-01-myvec-core.rs.
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
    start: usize,
    next: usize,
    end: usize,
    tail_len: usize,
}

impl<T> MyVec<T> {
    pub fn drain(&mut self, range: Range<usize>) -> Drain<'_, T> {
        let Range { start, end } = range;
        assert!(start <= end && end <= self.len, "drain range out of bounds");
        let tail_len = self.len - end;
        // BUG: self.len is left as it was. The Drain's destructor is trusted to repair it.
        Drain { vec: self, start, next: start, end, tail_len }
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
        Some(unsafe { self.vec.ptr().add(i).read() }) // moved out, but still inside vec[0, len)!
    }
}

impl<T> Drop for Drain<'_, T> {
    fn drop(&mut self) {
        // Correct IF it runs: drop the rest of the range, slide the tail down, fix len.
        for x in self.by_ref() {
            drop(x);
        }
        unsafe {
            let base = self.vec.ptr();
            ptr::copy(base.add(self.end), base.add(self.start), self.tail_len);
        }
        self.vec.len = self.start + self.tail_len;
    }
}

fn main() {
    let mut v: MyVec<String> = MyVec::new();
    for s in ["a", "b", "c"] {
        v.push(s.to_string());
    }
    let mut d = v.drain(0..2);
    let a = d.next(); // "a" is moved out to the caller...
    std::mem::forget(d); // ...and the repair in Drop never runs. No `unsafe` in main.
    drop(a); // the caller frees "a"
    println!("v still claims len={}", v.len());
} // v drops slot 0 ("a") again: use after free
