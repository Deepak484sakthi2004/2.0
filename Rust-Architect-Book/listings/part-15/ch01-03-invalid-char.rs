// verify: debug miri char
// 0xD800 is a UTF-16 surrogate, not a Unicode scalar value: an invalid `char`.
fn main() {
    let c: char = unsafe { std::mem::transmute::<u32, char>(0xD800) };
    println!("{}", c as u32);
}
