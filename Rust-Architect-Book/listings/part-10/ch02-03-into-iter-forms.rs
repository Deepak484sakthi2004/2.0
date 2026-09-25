// verify: debug ok
// The three IntoIterator impls on Vec<T>, and what each `for` loop gets.
use std::any::type_name_of_val;

fn main() {
    let mut routes = vec!["/pay".to_string(), "/refund".to_string()];

    for r in &routes {
        // IntoIterator for &Vec<T>: items are &T, the Vec is only borrowed
        println!("for r in &routes      -> {}", type_name_of_val(&r));
    }
    for r in &mut routes {
        // IntoIterator for &mut Vec<T>: items are &mut T, edit in place
        r.push_str("/v2");
    }
    println!("after &mut: {routes:?}");
    for r in routes {
        // IntoIterator for Vec<T>: items are T (owned), the Vec is consumed
        println!("for r in routes       -> {}", type_name_of_val(&r));
    }
    // `routes` is gone here: its buffer was freed when the loop's IntoIter was dropped.

    // Arrays: by-value IntoIterator since edition 2021 (listing ch02-05 shows edition 2018).
    let codes = [200_u16, 404, 503];
    let first = codes.into_iter().next().unwrap();
    println!("[u16; 3].into_iter()  -> {}", type_name_of_val(&first));
}
