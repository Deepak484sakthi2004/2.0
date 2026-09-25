// verify: debug miri SharedReadOnly
// The same "counter", without UnsafeCell: writing through a pointer derived from `&u32`.
// The lint of Chapter 15.2 catches the obvious one-liner; a helper function hides it from the lint,
// but not from Miri.
struct Counter {
    hits: u32,
}

fn as_mut_ptr_from_shared<T>(r: &T) -> *mut T {
    r as *const T as *mut T
}

impl Counter {
    fn bump(&self) {
        let p = as_mut_ptr_from_shared(&self.hits);
        unsafe { *p += 1 }; // UB: this memory is behind a shared reference and not inside an UnsafeCell
    }
}

fn main() {
    let c = Counter { hits: 0 };
    c.bump();
    println!("{}", c.hits);
}
