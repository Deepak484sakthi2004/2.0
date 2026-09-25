// verify: debug+nightly ok
// verify: debug miri-ok
// The eyepatch (nightly): `#[may_dangle] T` promises Drop won't use T values except by dropping them.
#![feature(dropck_eyepatch)]
use std::marker::PhantomData;

struct MyVec<T> {
    ptr: *mut T,
    len: usize,
    cap: usize,
    _owns: PhantomData<T>, // keep this: it tells drop check we DO drop T values (see ch03-16/17)
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

// SAFETY: drop only DROPS the T values (through Vec's own drop); it never reads them otherwise.
unsafe impl<#[may_dangle] T> Drop for MyVec<T> {
    fn drop(&mut self) {
        // SAFETY: ptr/len/cap describe a buffer this MyVec owns; rebuilt exactly once, here.
        unsafe { drop(Vec::from_raw_parts(self.ptr, self.len, self.cap)) };
    }
}

fn main() {
    let mut names: MyVec<&String> = MyVec::new();
    let s = String::from("merchant-7");
    names.push(&s);
    println!("accepted: MyVec<&String> may outlive the String it points to, until it is dropped");
}
