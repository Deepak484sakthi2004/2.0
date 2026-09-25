// verify: debug ok
// verify: debug miri-ok
// Counting what MyVec asks the allocator for, next to std's Vec: growth for u64, nothing at all for a ZST.
// Core identical to ch04-01-myvec-core.rs; the counting allocator also counts realloc separately.
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


mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    static ALLOCS: AtomicUsize = AtomicUsize::new(0);
    static REALLOCS: AtomicUsize = AtomicUsize::new(0);
    static FREES: AtomicUsize = AtomicUsize::new(0);

    pub struct Counting;

    // SAFETY: every method forwards its exact arguments to `System`, which upholds the GlobalAlloc
    // contract; the counters are atomics, so counting never allocates or re-enters the allocator.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            REALLOCS.fetch_add(1, Relaxed);
            unsafe { System.realloc(ptr, layout, new_size) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            FREES.fetch_add(1, Relaxed);
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    #[global_allocator]
    static GLOBAL: Counting = Counting;

    /// (allocs, reallocs, frees) performed while running `f`.
    pub fn measure(f: impl FnOnce()) -> (usize, usize, usize) {
        let before = (ALLOCS.load(Relaxed), REALLOCS.load(Relaxed), FREES.load(Relaxed));
        f();
        (ALLOCS.load(Relaxed) - before.0, REALLOCS.load(Relaxed) - before.1, FREES.load(Relaxed) - before.2)
    }
}

fn main() {
    let n = 1000u64;
    let mine = counting::measure(|| {
        let mut v = MyVec::new();
        for i in 0..n {
            v.push(i);
        }
        assert_eq!(v.capacity(), 1024);
    });
    let std_vec = counting::measure(|| {
        let mut v = Vec::new();
        for i in 0..n {
            v.push(i);
        }
        assert_eq!(v.capacity(), 1024);
    });
    println!("u64 x {n}:  MyVec (allocs, reallocs, frees) = {mine:?}   std Vec = {std_vec:?}");

    let zst_mine = counting::measure(|| {
        let mut v = MyVec::new();
        for _ in 0..10_000 {
            v.push(());
        }
        assert_eq!((v.len(), v.capacity()), (10_000, usize::MAX));
    });
    let zst_std = counting::measure(|| {
        let mut v = Vec::new();
        for _ in 0..10_000 {
            v.push(());
        }
        assert_eq!((v.len(), v.capacity()), (10_000, usize::MAX));
    });
    println!("() x 10000: MyVec = {zst_mine:?}   std Vec = {zst_std:?}");
}
