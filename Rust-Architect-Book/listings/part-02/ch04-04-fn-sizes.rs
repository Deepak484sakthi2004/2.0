// verify: debug ok
use std::mem::size_of_val;

fn double(x: u64) -> u64 {
    x * 2
}

fn main() {
    let item = double; // a function ITEM: a unique, zero-sized type
    let ptr: fn(u64) -> u64 = double; // a function POINTER: an address
    let factor = 3;
    let closure = move |x: u64| x * factor; // a closure: a struct holding its captures
    println!("fn item:    {} bytes", size_of_val(&item));
    println!("fn pointer: {} bytes", size_of_val(&ptr));
    println!("closure:    {} bytes (captures one u64)", size_of_val(&closure));
    println!("unit ():    {} bytes", size_of_val(&()));
    println!("results:    {} {} {}", item(21), ptr(21), closure(14));
}
