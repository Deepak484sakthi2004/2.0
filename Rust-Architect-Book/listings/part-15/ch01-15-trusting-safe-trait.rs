// verify: debug crash precondition
// verify: debug miri Undefined
// UNSOUND: unsafe code that trusts a SAFE trait's promise. `ExactSizeIterator::len` is an ordinary
// safe method; a wrong implementation is a bug in *its* code, but the UB lands in *ours*.
fn collect_exact<I: ExactSizeIterator<Item = u64>>(it: I) -> Vec<u64> {
    let n = it.len();
    let mut out: Vec<u64> = Vec::with_capacity(n);
    let p = out.as_mut_ptr();
    let mut written = 0;
    for x in it {
        unsafe { p.add(written).write(x) }; // no bounds check: "len() told us"
        written += 1;
    }
    unsafe { out.set_len(written) };
    out
}

/// A safe, wrong iterator: claims 2 items, yields 4. No `unsafe` in it anywhere.
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
    let v = collect_exact(Liar(0));
    println!("{v:?}");
}
