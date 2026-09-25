// verify: debug error:E0499
// The price of lending: you can't hold two items at once (so no collect(), no windows, no zip of self).
use std::io::BufRead;

trait LendingIterator {
    type Item<'a>
    where
        Self: 'a;
    fn next(&mut self) -> Option<Self::Item<'_>>;
}

struct Lines<R> {
    src: R,
    buf: Vec<u8>,
}

impl<R: BufRead> LendingIterator for Lines<R> {
    type Item<'a>
        = &'a [u8]
    where
        Self: 'a;

    fn next(&mut self) -> Option<&[u8]> {
        self.buf.clear();
        match self.src.read_until(b'\n', &mut self.buf) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(&self.buf),
        }
    }
}

fn main() {
    let mut lines = Lines { src: &b"a\nb\n"[..], buf: Vec::new() };
    let first = lines.next();
    let second = lines.next(); // the buffer `first` points into is about to be overwritten
    println!("{first:?} {second:?}");
}
