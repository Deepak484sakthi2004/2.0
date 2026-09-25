// verify: debug miri uninitialized
// set_len BEFORE initialization: "read() will fill it"... but a short read doesn't.
use std::io::Read;

fn checksum_frame(src: &mut impl Read, n: usize) -> u32 {
    let mut buf: Vec<u8> = Vec::with_capacity(n);
    unsafe { buf.set_len(n) }; // BUG: claims n initialized bytes before any are written
    let _got = src.read(&mut buf).unwrap(); // a short read initializes only `_got` of them
    buf.iter().map(|&b| b as u32).sum() // reads the rest: uninitialized
}

fn main() {
    let mut src: &[u8] = &[1, 2, 3];
    println!("{}", checksum_frame(&mut src, 8));
}
