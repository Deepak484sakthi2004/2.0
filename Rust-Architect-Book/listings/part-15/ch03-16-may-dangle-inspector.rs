// verify: debug+nightly error:E0597
// With PhantomData<T>, drop check still protects T's OWN destructor: an element that reads its
// reference in Drop must not outlive the referent, even inside a #[may_dangle] container.
#![feature(dropck_eyepatch)]
use std::marker::PhantomData;

struct MyVec<T> {
    ptr: *mut T,
    len: usize,
    cap: usize,
    _owns: PhantomData<T>,
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

unsafe impl<#[may_dangle] T> Drop for MyVec<T> {
    fn drop(&mut self) {
        // SAFETY: ptr/len/cap describe a buffer this MyVec owns; rebuilt exactly once, here.
        unsafe { drop(Vec::from_raw_parts(self.ptr, self.len, self.cap)) };
    }
}

/// An element whose destructor READS what it points to.
struct AuditOnDrop<'a>(&'a String);
impl Drop for AuditOnDrop<'_> {
    fn drop(&mut self) {
        println!("audit: released {}", self.0);
    }
}

fn main() {
    let mut v: MyVec<AuditOnDrop<'_>> = MyVec::new();
    let s = String::from("merchant-7");
    v.push(AuditOnDrop(&s));
}
