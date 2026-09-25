// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target asm -Mode release
// A frame bigger than one 4 KiB page must touch each page in order, or it could jump over the guard page.

#[inline(never)]
pub fn small_frame(x: u64) -> u64 {
    let buf = [x; 64]; // 512 bytes: no probe needed
    std::hint::black_box(&buf);
    buf[7]
}

#[inline(never)]
pub fn big_frame(x: u64) -> u64 {
    let buf = [x; 2048]; // 16 KiB: spans four pages
    std::hint::black_box(&buf);
    buf[7]
}
