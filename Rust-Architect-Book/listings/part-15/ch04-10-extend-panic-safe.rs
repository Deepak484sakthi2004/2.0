// verify: debug ok
// verify: debug miri-ok
// Panic-safe extend_from_slice: a guard publishes the length written SO FAR when it is dropped,
// including during unwinding (std's Vec uses the same idea, SetLenOnDrop). Core identical to ch04-01.
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


impl<T> MyVec<T> {
    pub fn reserve(&mut self, additional: usize) {
        while self.buf.cap - self.len < additional {
            self.buf.grow();
        }
    }
}

/// A value whose Clone panics for one input: stands in for any user Clone impl, which may panic.
struct Flaky(String);
impl Clone for Flaky {
    fn clone(&self) -> Self {
        if self.0 == "boom" {
            panic!("clone of {:?} failed", self.0);
        }
        Flaky(self.0.clone())
    }
}

fn flaky(items: &[&str]) -> Vec<Flaky> {
    items.iter().map(|s| Flaky(s.to_string())).collect()
}

/// Writes the element count back into the vector when dropped, even during a panic.
struct SetLenOnDrop<'a> {
    len: &'a mut usize,
    local_len: usize,
}
impl Drop for SetLenOnDrop<'_> {
    fn drop(&mut self) {
        *self.len = self.local_len;
    }
}

impl<T: Clone> MyVec<T> {
    pub fn extend_from_slice(&mut self, items: &[T]) {
        self.reserve(items.len());
        let base = self.ptr();
        let mut guard = SetLenOnDrop { local_len: self.len, len: &mut self.len };
        for item in items {
            let value = item.clone(); // may panic: the guard then publishes only the slots written
            // SAFETY: reserve() made room for items.len() more; slot local_len is uninitialized.
            unsafe { base.add(guard.local_len).write(value) };
            guard.local_len += 1;
        }
    } // guard dropped: len = everything written
}

fn main() {
    let mut v: MyVec<Flaky> = MyVec::new();
    v.push(Flaky("a".to_string()));
    let batch = flaky(&["b", "boom", "c"]);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| v.extend_from_slice(&batch)));
    let names: Vec<&str> = v.iter().map(|f| f.0.as_str()).collect();
    println!("caught panic: {}, contents now {:?}", result.is_err(), names);
    v.extend_from_slice(&flaky(&["c", "d"]));
    println!("len after a clean extend: {}", v.len());
}
