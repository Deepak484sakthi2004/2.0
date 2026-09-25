// verify: debug error:E0277
// UnsafeCell makes MyCell !Sync automatically: sharing it between threads doesn't compile.
use std::cell::UnsafeCell;

pub struct MyCell<T> {
    value: UnsafeCell<T>,
}

impl<T: Copy> MyCell<T> {
    pub fn get(&self) -> T {
        // SAFETY: single-threaded by construction (!Sync), no references into the interior.
        unsafe { *self.value.get() }
    }
}

fn main() {
    let c = MyCell { value: UnsafeCell::new(0u32) };
    std::thread::scope(|s| {
        s.spawn(|| c.get());
        s.spawn(|| c.get());
    });
}
