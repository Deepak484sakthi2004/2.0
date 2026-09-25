// verify: debug ok
// verify: debug miri dangling
//! A self-referential struct without Pin: `cursor` points into `buf`, and the first move of the
//! struct leaves it pointing at the OLD location. The native run only compares addresses;
//! Miri also dereferences the stale pointer and reports the undefined behavior.

struct Parser {
    buf: [u8; 16],
    cursor: *const u8, // points INTO `buf`: a self-reference
}

impl Parser {
    fn new(src: &[u8]) -> Parser {
        let mut p = Parser { buf: [0; 16], cursor: std::ptr::null() };
        p.buf[..src.len()].copy_from_slice(src);
        p.cursor = p.buf.as_ptr(); // valid... until `p` moves
        p // returning moves it: `cursor` now points into this dead stack frame
    }

    fn peek(&self) -> u8 {
        // SAFETY: claims `cursor` points into `self.buf`. That's false after any move of `self`,
        // and nothing in the type system prevents the move. This is the bug.
        unsafe { *self.cursor }
    }
}

fn main() {
    let p = Parser::new(b"GET /");
    let boxed = Box::new(p); // and another move, to the heap
    println!("cursor points into buf: {}", boxed.cursor == boxed.buf.as_ptr());
    if cfg!(miri) {
        println!("peek: {}", boxed.peek() as char); // reads a dead stack slot
    }
}
