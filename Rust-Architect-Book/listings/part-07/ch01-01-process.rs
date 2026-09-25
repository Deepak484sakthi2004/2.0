// verify: debug ok
use std::any::type_name;
use std::fmt::Debug;
use std::mem::size_of;

#[derive(Debug)]
#[allow(dead_code)]
struct MyType {
    id: u32,
    tags: Vec<&'static str>,
}

/// One generic definition. The compiler generates one copy per concrete T it is used with.
fn process<T: Debug>(x: T) -> usize {
    let text = format!("{x:?}");
    println!(
        "process::<{}>  size_of::<T>() = {:>2}  value = {text}",
        type_name::<T>(),
        size_of::<T>()
    );
    text.len()
} // `x` is dropped here: for String and MyType that frees heap memory; for i32 it is nothing

fn main() {
    let total = process(42_i32) // T = i32, inferred from the argument
        + process(String::from("hello")) // T = String
        + process(MyType { id: 7, tags: vec!["vip"] }) // T = MyType
        + process::<u8>(255); // T = u8, written explicitly with the "turbofish"
    println!("total debug length = {total}");
}
