// verify: debug ok
use std::marker::PhantomData;

/// The same typed ID, with hand-written impls that put NO bound on the tag type.
struct Id<T> {
    raw: u64,
    _tag: PhantomData<T>,
}

impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Id<T> {}

struct Merchant;

fn main() {
    let a: Id<Merchant> = Id { raw: 7, _tag: PhantomData };
    let b = a; // Copy
    let c = a.clone();
    println!("{} {} {}", a.raw, b.raw, c.raw);
}
