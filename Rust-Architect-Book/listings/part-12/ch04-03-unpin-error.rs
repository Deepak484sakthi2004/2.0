// verify: debug error:E0277
//! Safe code can't get `&mut T` out of `Pin<&mut T>` when T is !Unpin, so it can't move T
//! (mem::swap, mem::replace, *slot = new all need `&mut T`).
use std::marker::PhantomPinned;
use std::pin::Pin;

struct Parser {
    buf: [u8; 16],
    _pin: PhantomPinned,
}

fn main() {
    let mut a = Box::pin(Parser { buf: [1; 16], _pin: PhantomPinned });
    let b = Parser { buf: [2; 16], _pin: PhantomPinned };
    let slot: &mut Parser = Pin::get_mut(a.as_mut()); // would allow `*slot = b`, a move
    *slot = b;
    println!("{}", a.buf[0]);
}
