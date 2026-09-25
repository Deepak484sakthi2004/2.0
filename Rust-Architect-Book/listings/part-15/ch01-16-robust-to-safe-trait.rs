// verify: debug ok
// verify: debug miri-ok
// SOUND: use len() only as a HINT; never let a safe trait's answer decide what memory we touch.
fn collect_exact<I: ExactSizeIterator<Item = u64>>(it: I) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::with_capacity(it.len()); // a hint: wrong only costs a realloc
    for x in it {
        out.push(x); // bounds are checked against the Vec's OWN capacity
    }
    out
}

struct Liar(u64);
impl Iterator for Liar {
    type Item = u64;
    fn next(&mut self) -> Option<u64> {
        self.0 += 1;
        (self.0 <= 4).then_some(self.0)
    }
}
impl ExactSizeIterator for Liar {
    fn len(&self) -> usize {
        2
    }
}

fn main() {
    println!("{:?}", collect_exact(Liar(0)));
}
