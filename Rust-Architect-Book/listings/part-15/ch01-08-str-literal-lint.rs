// verify: debug error:invalid_from_utf8_unchecked
// When the invalid bytes are a literal, a deny-by-default lint catches it at compile time.
fn main() {
    let bytes = [b'o', b'k', 0xFF, 0xFE];
    let s: &str = unsafe { std::str::from_utf8_unchecked(&bytes) };
    println!("{s}");
}
