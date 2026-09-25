// verify: release ok
// verify: debug crash misaligned
// verify: debug miri alignment
// Dereferencing a misaligned *const u32 is UB in Rust even though x86-64 loads it happily.
fn main() {
    let words = std::hint::black_box([0x0403_0201u32, 0x0807_0605, 0, 0]); // 4-aligned storage
    let p = (words.as_ptr() as *const u8).wrapping_add(1) as *const u32; // aligned + 1
    let v = unsafe { *p };
    println!("{v:#010x}");
}
