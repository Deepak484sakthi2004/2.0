// verify: debug ok
use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use std::mem::size_of;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards every call to the System allocator unchanged; only counts calls.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn allocs_during<R>(f: impl FnOnce() -> R) -> (R, usize) {
    let before = ALLOCS.load(Relaxed);
    let r = f();
    (r, ALLOCS.load(Relaxed) - before)
}

struct Audited; // a unit struct: one value, zero bytes

fn main() {
    println!("size_of: ()={} Audited={} PhantomData<String>={} [Audited; 1000]={}",
        size_of::<()>(), size_of::<Audited>(), size_of::<PhantomData<String>>(), size_of::<[Audited; 1000]>());

    let (v, n) = allocs_during(|| {
        let mut v: Vec<Audited> = Vec::new();
        for _ in 0..1_000_000 {
            v.push(Audited);
        }
        v
    });
    println!("Vec<Audited>: len={} capacity={} allocations={}", v.len(), v.capacity(), n);

    let (v2, n2) = allocs_during(|| {
        let mut v: Vec<u8> = Vec::new();
        for _ in 0..1_000_000 {
            v.push(0);
        }
        v
    });
    println!("Vec<u8>:      len={} capacity={} allocations={}", v2.len(), v2.capacity(), n2);

    // HashSet<K> is a HashMap<K, ()>: the value slot costs nothing.
    println!("size_of::<(u64, ())>()={}  HashSet<u64>={} HashMap<u64,()>={}",
        size_of::<(u64, ())>(), size_of::<HashSet<u64>>(), size_of::<HashMap<u64, ()>>());
}
