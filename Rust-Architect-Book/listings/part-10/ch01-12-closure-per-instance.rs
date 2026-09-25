// verify: debug ok
// A closure written inside a generic function is generic too: one closure TYPE per instance
// of the enclosing function (Chapter 7.3 relied on this).
use std::any::type_name_of_val;

fn describe<T: std::fmt::Debug>(items: &[T]) -> Vec<String> {
    let fmt_one = |item: &T| format!("{item:?}");
    println!("closure type: {}", type_name_of_val(&fmt_one));
    items.iter().map(fmt_one).collect()
}

fn main() {
    println!("{:?}", describe(&[1u8, 2]));
    println!("{:?}", describe(&["a", "b"]));
    println!("{:?}", describe(&[1.5f64]));
}
