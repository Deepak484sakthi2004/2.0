// verify: debug ok
// Listing 17.5-4: where Rust's inference gets its information. Constraints flow forward AND
// backward within a function body; unconstrained literals fall back to i32 / f64.
use std::any::type_name_of_val;

fn main() {
    let a = 42; // {integer}, nothing else constrains it -> falls back to i32
    let b = 2.5; // {float} -> f64
    let c = 7; // {integer} ...
    let d: u64 = c; // ... fixed by a LATER use: c is u64
    let mut v = Vec::new(); // Vec<?T> ...
    v.push(1u8); // ... ?T = u8, from a method call two lines later
    let parsed: Vec<u16> = "1,2,3".split(',').map(|s| s.parse().unwrap()).collect(); // parse::<u16>, from the annotation
    println!("a: {}", type_name_of_val(&a));
    println!("b: {}", type_name_of_val(&b));
    println!("c: {} (d = {d})", type_name_of_val(&c));
    println!("v: {}", type_name_of_val(&v));
    println!("parsed: {} = {parsed:?}", type_name_of_val(&parsed));
}
