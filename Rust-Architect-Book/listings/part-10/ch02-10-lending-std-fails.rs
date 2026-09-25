// verify: debug error:E0207
// Attempt: a line reader that yields slices of its OWN reused buffer through std's Iterator.
use std::io::BufRead;

struct Lines<R> {
    src: R,
    buf: Vec<u8>,
}

impl<'a, R: BufRead> Iterator for Lines<R> {
    type Item = &'a [u8]; // borrows from self.buf... but which self? `'a` is tied to nothing
    fn next(&mut self) -> Option<&'a [u8]> {
        self.buf.clear();
        match self.src.read_until(b'\n', &mut self.buf) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(&self.buf),
        }
    }
}

fn main() {}
