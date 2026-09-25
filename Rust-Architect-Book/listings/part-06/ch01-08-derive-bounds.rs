// verify: debug error:E0599
use std::marker::PhantomData;

/// A typed ID (Part V style): the u64 is the data; T is only a compile-time tag.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Id<T> {
    raw: u64,
    _tag: PhantomData<T>,
}

/// A tag type that is deliberately not Clone.
struct Merchant;

fn main() {
    let a: Id<Merchant> = Id { raw: 7, _tag: PhantomData };
    let b = a.clone(); // derive generated `impl<T: Clone> Clone for Id<T>`: requires Merchant: Clone
    println!("{}", b.raw);
}
