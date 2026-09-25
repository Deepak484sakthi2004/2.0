// verify: debug ok
use std::mem;

fn main() {
    let mut names = vec![String::from("ada"), String::from("grace"), String::from("linus")];

    let borrowed: &String = &names[0]; // borrow: nothing moves
    println!("borrowed {borrowed}");

    let taken = mem::take(&mut names[1]); // move out, leave String::default() behind
    let replaced = mem::replace(&mut names[2], String::from("ken")); // move out, put a value back
    let removed = names.swap_remove(0); // move out of the Vec itself (O(1); last element fills the gap)
    println!("taken={taken} replaced={replaced} removed={removed} left={names:?}");

    let mut slot: Option<String> = Some(String::from("session-42"));
    let owned = slot.take(); // Option::take: move out, leave None
    println!("owned={owned:?} slot={slot:?}");
}
