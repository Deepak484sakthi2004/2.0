// verify: debug ok
// verify: debug miri-ok
//! Cell, rebuilt on the one primitive: UnsafeCell. The safety argument is the whole design.
use std::cell::UnsafeCell;

pub struct MyCell<T> {
    value: UnsafeCell<T>,
}

impl<T: Copy> MyCell<T> {
    pub fn new(value: T) -> Self {
        MyCell { value: UnsafeCell::new(value) }
    }

    pub fn get(&self) -> T {
        // SAFETY: MyCell is !Sync (UnsafeCell is !Sync, and we add no impl), so only this thread can reach it.
        // No reference into the interior is ever handed out, so no `&T` can observe the write in `set`.
        unsafe { *self.value.get() }
    }

    pub fn set(&self, value: T) {
        // SAFETY: as in `get`: single thread, and nobody holds a reference to the interior.
        unsafe { *self.value.get() = value }
    }
}

fn main() {
    let hits = MyCell::new(0u32);
    let a = &hits;
    let b = &hits; // two shared references...
    a.set(a.get() + 1);
    b.set(b.get() + 1); // ...both mutating: legal, because the mutation goes through UnsafeCell
    println!("hits = {}", hits.get());
}
