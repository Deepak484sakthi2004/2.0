// verify: debug error:E0658
// The one operation that justifies a linked list, O(1) removal at a position you already hold,
// needs the cursor API. On stable Rust 1.98 it is still feature-gated.
use std::collections::LinkedList;

fn main() {
    let mut orders: LinkedList<u64> = (1..=5).collect();
    let mut cursor = orders.cursor_front_mut();
    cursor.move_next(); // now at 2
    cursor.remove_current(); // O(1) unlink
    println!("{orders:?}");
}
