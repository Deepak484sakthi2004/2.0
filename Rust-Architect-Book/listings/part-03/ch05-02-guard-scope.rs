// verify: debug ok
// verify: debug@2021 ok
use std::sync::Mutex;

fn state<T>(m: &Mutex<T>) -> &'static str {
    // try_lock never blocks: it tells us whether the lock is currently held
    if m.try_lock().is_ok() { "free" } else { "STILL LOCKED" }
}

fn main() {
    let queue = Mutex::new(vec![1, 2, 3]);

    // A temporary created in a `match` scrutinee lives until the END of the match.
    match queue.lock().unwrap().len() {
        n => println!("inside match (len {n}):       {}", state(&queue)),
    }

    // Binding the value first ends the temporary at the end of the `let` statement.
    let n = queue.lock().unwrap().len();
    println!("after `let n = ...` (len {n}): {}", state(&queue));

    // `if let`: the guard lives through the body...
    if let Some(&first) = queue.lock().unwrap().first() {
        println!("inside if-let body ({first}):  {}", state(&queue));
    }

    // ...but in edition 2024 it is dropped BEFORE the `else` block (in 2021 it was still held).
    if let Some(&x) = queue.lock().unwrap().get(99) {
        println!("unreachable {x}");
    } else {
        println!("inside if-let else:        {}", state(&queue));
    }
    println!("done"); // keeps the `if let` from being main's tail expression (see ch05-09)
}
