// verify: debug ok
use std::cell::Cell;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

/// A smart pointer that counts how often its contents are accessed.
struct Tracked<T> {
    value: T,
    reads: Cell<u32>,
}

impl<T> Tracked<T> {
    fn new(value: T) -> Self {
        Tracked { value, reads: Cell::new(0) }
    }
}

impl<T> Deref for Tracked<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.reads.set(self.reads.get() + 1);
        &self.value
    }
}

impl<T> DerefMut for Tracked<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

fn shout(text: &str) -> String {
    text.to_uppercase()
}

fn main() {
    // Auto-deref in method calls: `len` is a method of str, found through Box -> String -> str.
    let boxed: Box<String> = Box::new(String::from("gateway"));
    let shared: Rc<String> = Rc::new(String::from("ledger"));
    println!("boxed.len() = {}, shared.len() = {}", boxed.len(), shared.len());

    // Deref coercion at a call site: &Box<String> -> &String -> &str.
    println!("{}", shout(&boxed));

    // Our own smart pointer participates in the same machinery.
    let mut name = Tracked::new(String::from("meridian"));
    name.push_str("-eu"); // DerefMut: &mut Tracked<String> -> &mut String
    let n = name.len(); // Deref (counted)
    let upper = shout(&name); // deref coercion &Tracked<String> -> &String -> &str (counted)
    println!("{upper} ({n} bytes), reads through Deref = {}", name.reads.get());
}
