// verify: debug panic uninitialized
// For types where 0x01 bytes are still invalid (references, NonZero, ...), it panics instead.
#![allow(deprecated, invalid_value)]

fn main() {
    let r: &u64 = unsafe { std::mem::uninitialized() };
    println!("{r}");
}
