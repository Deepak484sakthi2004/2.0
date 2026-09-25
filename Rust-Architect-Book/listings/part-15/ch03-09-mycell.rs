// verify: debug ok
// verify: debug miri-ok
// A minimal Cell<T> over UnsafeCell: the ONLY sanctioned way to mutate behind a shared reference.
use std::cell::UnsafeCell;
use std::num::NonZeroU32;

pub struct MyCell<T> {
    value: UnsafeCell<T>,
}

impl<T: Copy> MyCell<T> {
    pub fn new(v: T) -> Self {
        MyCell { value: UnsafeCell::new(v) }
    }
    pub fn get(&self) -> T {
        // SAFETY: MyCell is !Sync (UnsafeCell is !Sync), so no other thread can access it; and we
        // never hand out references into the cell, so no reference can observe this read racing a write.
        unsafe { *self.value.get() }
    }
    pub fn set(&self, v: T) {
        // SAFETY: as in `get`: single-threaded, and no outstanding reference to the interior exists.
        unsafe { *self.value.get() = v }
    }
}

fn main() {
    let hits = MyCell::new(0u32);
    let r1 = &hits;
    let r2 = &hits; // two shared references, both able to mutate
    r1.set(r1.get() + 1);
    r2.set(r2.get() + 1);
    println!("hits = {}", hits.get());
    println!(
        "Option<NonZeroU32> = {} B, Option<UnsafeCell<NonZeroU32>> = {} B (UnsafeCell hides the niche)",
        size_of::<Option<NonZeroU32>>(),
        size_of::<Option<UnsafeCell<NonZeroU32>>>()
    );
}
