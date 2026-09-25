// verify: debug ok
// verify: debug miri-ok
// verify: debug+tree miri-ok
//! The same parser, pinned. PhantomPinned makes it !Unpin; construction happens inside a
//! Pin<Box<_>>, and every method takes Pin<&Self> or Pin<&mut Self>, so safe code can never move it.
use std::marker::PhantomPinned;
use std::pin::Pin;

struct Parser {
    buf: [u8; 16],
    len: usize,
    cursor: *const u8,
    _pin: PhantomPinned,
}

impl Parser {
    fn new(src: &[u8]) -> Pin<Box<Parser>> {
        assert!(src.len() <= 16);
        let mut p = Box::pin(Parser { buf: [0; 16], len: src.len(), cursor: std::ptr::null(), _pin: PhantomPinned });
        // SAFETY: we only write fields; nothing is moved out of the pinned Parser.
        let this = unsafe { p.as_mut().get_unchecked_mut() };
        this.buf[..src.len()].copy_from_slice(src);
        this.cursor = this.buf.as_ptr(); // set AFTER pinning: the address is now final
        p
    }

    fn peek(self: Pin<&Self>) -> Option<u8> {
        let offset = self.cursor as usize - self.buf.as_ptr() as usize;
        // SAFETY: `cursor` was derived from `buf` after the Parser was pinned; a pinned Parser never
        // moves, so the pointer is valid while `self` is. `offset < len` keeps the read in bounds.
        (offset < self.len).then(|| unsafe { *self.cursor })
    }

    fn advance(self: Pin<&mut Self>) {
        // SAFETY: we update a plain field; nothing is moved out.
        let this = unsafe { self.get_unchecked_mut() };
        // SAFETY: stays within (or one past the end of) `buf`, checked by `peek` before any read.
        this.cursor = unsafe { this.cursor.add(1) };
    }
}

fn main() {
    let p = Parser::new(b"GET /");
    let mut parsers = vec![p]; // moves the Box (a pointer), not the pinned Parser
    let p = &mut parsers[0];
    println!("cursor points into buf: {}", p.cursor == p.buf.as_ptr());
    let mut seen = String::new();
    while let Some(b) = p.as_ref().peek() {
        seen.push(b as char);
        p.as_mut().advance();
    }
    println!("parsed {seen:?}");
}
