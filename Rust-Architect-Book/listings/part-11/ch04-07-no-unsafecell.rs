// verify: debug miri SharedReadOnly
//! The same "cell" WITHOUT UnsafeCell: writing through a pointer derived from `&T`.
//! It compiles and appears to work; Miri reports it as Undefined Behavior.
pub struct BadCell<T> {
    value: T,
}

impl<T: Copy> BadCell<T> {
    pub fn get(&self) -> T {
        self.value
    }

    pub fn set(&self, value: T) {
        let p: *mut T = (&raw const self.value).cast_mut();
        // SAFETY: none. `&self` promises the value is frozen; writing through it is UB.
        unsafe { p.write(value) }
    }
}

fn main() {
    let hits = BadCell { value: 0u32 };
    hits.set(hits.get() + 1);
    println!("hits = {}", hits.get());
}
