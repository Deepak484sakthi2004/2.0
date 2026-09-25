// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target mir -Mode debug
#[inline(never)]
pub fn consume(s: String) -> usize {
    std::hint::black_box(s).len()
}

pub fn maybe_consume(flag: bool) {
    let s = String::from("hi");
    if flag {
        consume(s);
    }
} // `s` must be dropped here ONLY if it was not moved: the compiler needs a drop flag
