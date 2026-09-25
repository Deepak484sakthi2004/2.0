// verify: release build
// Reading into a pooled buffer through `Read`: the buffer must be initialized first, so it is zeroed.
use std::io::Read;

#[inline(never)]
pub fn fill(r: &mut dyn Read, buf: &mut Vec<u8>, n: usize) -> std::io::Result<usize> {
    buf.clear();
    buf.resize(n, 0); // Read::read takes &mut [u8]: initialized memory, even though read() overwrites it
    r.read(buf)
}
