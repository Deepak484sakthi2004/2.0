// verify: debug ok
// Same fields as ch04-01's MyVec, next to std's Vec: which one leaves spare bit patterns (niches) for enums?
use std::marker::PhantomData;
use std::mem::size_of;
use std::ptr::NonNull;

#[allow(dead_code)]
struct RawVec<T> {
    ptr: NonNull<T>, // niche: null
    cap: usize,      // no niche: every usize is a valid capacity as far as the compiler knows
    _owns: PhantomData<T>,
}

#[allow(dead_code)]
pub struct MyVec<T> {
    buf: RawVec<T>,
    len: usize,
}

fn main() {
    println!("MyVec<u8>                 {:2} bytes", size_of::<MyVec<u8>>());
    println!("Option<MyVec<u8>>         {:2} bytes", size_of::<Option<MyVec<u8>>>());
    println!("Option<Option<MyVec<u8>>> {:2} bytes", size_of::<Option<Option<MyVec<u8>>>>());
    println!("Vec<u8>                   {:2} bytes", size_of::<Vec<u8>>());
    println!("Option<Vec<u8>>           {:2} bytes", size_of::<Option<Vec<u8>>>());
    println!("Option<Option<Vec<u8>>>   {:2} bytes", size_of::<Option<Option<Vec<u8>>>>());
    println!("Result<Vec<u8>, u32>      {:2} bytes", size_of::<Result<Vec<u8>, u32>>());
}
