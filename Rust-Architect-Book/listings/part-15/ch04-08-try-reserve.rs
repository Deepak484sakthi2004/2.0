// verify: debug ok
// Fallible allocation: try_reserve reports "too big to describe" (CapacityOverflow) and "the allocator
// said no" (AllocError) as values instead of aborting. A failed realloc leaves the old buffer intact.
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


#[derive(Debug)]
pub enum TryReserveError {
    /// The request can't even be expressed: len + additional overflows, or the byte size > isize::MAX.
    CapacityOverflow,
    /// A valid request that the allocator refused (returned null).
    AllocError { size: usize, align: usize },
}

impl<T> MyVec<T> {
    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        use TryReserveError::*;
        let needed = self.len.checked_add(additional).ok_or(CapacityOverflow)?;
        if needed <= self.buf.cap {
            return Ok(()); // also the ZST case: cap == usize::MAX
        }
        let new_cap = needed.max(self.buf.cap.saturating_mul(2)).max(4);
        let new_layout = Layout::array::<T>(new_cap).map_err(|_| CapacityOverflow)?;
        let new_ptr = if self.buf.cap == 0 {
            // SAFETY: non-zero size: T is not a ZST (else we returned above) and new_cap >= 4.
            unsafe { alloc::alloc(new_layout) }
        } else {
            let old_layout = Layout::array::<T>(self.buf.cap).unwrap();
            // SAFETY: same contract as in grow(). If realloc returns null, the old block is untouched
            // and still owned by us (GlobalAlloc::realloc's documented contract).
            unsafe { alloc::realloc(self.ptr().cast::<u8>(), old_layout, new_layout.size()) }
        };
        match NonNull::new(new_ptr.cast::<T>()) {
            Some(p) => {
                self.buf.ptr = p;
                self.buf.cap = new_cap;
                Ok(())
            }
            None => Err(AllocError { size: new_layout.size(), align: new_layout.align() }),
        }
    }
}

fn main() {
    let mut v: MyVec<u64> = MyVec::new();
    for x in [7, 8, 9] {
        v.push(x);
    }
    println!("usize::MAX more  -> {:?}", v.try_reserve(usize::MAX));
    println!("2^61 more (2^64 bytes)  -> {:?}", v.try_reserve(1 << 61));
    println!("2^58 more (2^61 bytes)  -> {:?}", v.try_reserve(1 << 58));
    println!("after the failures: {:?} capacity={}", &v[..], v.capacity());
    println!("100 more -> {:?}, capacity={}", v.try_reserve(100), v.capacity());

    let mut s: Vec<u64> = Vec::new();
    match s.try_reserve(1 << 58) {
        Ok(()) => println!("std: reserved?!"),
        Err(e) => println!("std: {e}\nstd: {e:?}"),
    }
}
