// verify: debug error:E0597
// Drop check: a container with a plain `impl Drop` may, as far as the compiler knows, use its
// elements (here: references) inside `drop`. So they must outlive the container.
use std::marker::PhantomData;

struct MyVec<T> {
    ptr: *mut T,
    len: usize,
    cap: usize,
    _owns: PhantomData<T>, // "MyVec owns T values": dropping a MyVec<T> may drop T's
}

impl<T> MyVec<T> {
    fn new() -> Self {
        MyVec { ptr: std::ptr::NonNull::dangling().as_ptr(), len: 0, cap: 0, _owns: PhantomData }
    }
    fn push(&mut self, v: T) {
        // SAFETY: ptr/len/cap always describe a live Vec<T> buffer (or the empty dangling state).
        let mut vec = unsafe { Vec::from_raw_parts(self.ptr, self.len, self.cap) };
        vec.push(v);
        let mut vec = std::mem::ManuallyDrop::new(vec);
        (self.ptr, self.len, self.cap) = (vec.as_mut_ptr(), vec.len(), vec.capacity());
    }
}

impl<T> Drop for MyVec<T> {
    fn drop(&mut self) {
        // SAFETY: ptr/len/cap describe a buffer this MyVec owns; rebuilt exactly once, here.
        unsafe { drop(Vec::from_raw_parts(self.ptr, self.len, self.cap)) };
    }
}

fn main() {
    let mut names: MyVec<&String> = MyVec::new();
    let s = String::from("merchant-7"); // declared after `names`, so dropped BEFORE `names`
    names.push(&s);
} // error: `s` dropped here while still borrowed... borrow might be used when `names` is dropped
