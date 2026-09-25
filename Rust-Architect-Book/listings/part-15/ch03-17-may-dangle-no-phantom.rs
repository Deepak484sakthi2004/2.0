// verify: debug+nightly build
// verify: debug miri use-after-free
// UNSOUND: #[may_dangle] WITHOUT PhantomData<T>. Drop check no longer knows MyVec drops T values,
// so it accepts a program in which an element's destructor reads a String that is already freed.
#![feature(dropck_eyepatch)]

pub struct MyVec<T> {
    ptr: *mut T, // a raw pointer does not "own" T as far as drop check is concerned
    len: usize,
    cap: usize,
}

impl<T> MyVec<T> {
    pub fn new() -> Self {
        MyVec { ptr: std::ptr::NonNull::dangling().as_ptr(), len: 0, cap: 0 }
    }
    pub fn push(&mut self, v: T) {
        let mut vec = unsafe { Vec::from_raw_parts(self.ptr, self.len, self.cap) };
        vec.push(v);
        let mut vec = std::mem::ManuallyDrop::new(vec);
        (self.ptr, self.len, self.cap) = (vec.as_mut_ptr(), vec.len(), vec.capacity());
    }
}

unsafe impl<#[may_dangle] T> Drop for MyVec<T> {
    fn drop(&mut self) {
        unsafe { drop(Vec::from_raw_parts(self.ptr, self.len, self.cap)) };
    }
}

pub struct AuditOnDrop<'a>(pub &'a String);
impl Drop for AuditOnDrop<'_> {
    fn drop(&mut self) {
        println!("audit: released {}", self.0);
    }
}

pub fn main() {
    let mut v: MyVec<AuditOnDrop<'_>> = MyVec::new();
    let s = String::from("merchant-7");
    v.push(AuditOnDrop(&s));
} // `s` is freed first; then v's drop runs AuditOnDrop::drop, which reads the freed String
