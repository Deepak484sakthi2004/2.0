// verify: debug ok
use std::mem::size_of;

fn shout(text: &str) -> String {
    // accepts &str: works for String, literals, and slices alike
    text.to_uppercase()
}

fn main() {
    println!(
        "sizes: String={} &str={} Box<str>={} Vec<u8>={} &[u8]={}",
        size_of::<String>(), size_of::<&str>(), size_of::<Box<str>>(), size_of::<Vec<u8>>(), size_of::<&[u8]>()
    );

    let s = String::from("héllo wörld");
    println!("len() = {} bytes, chars().count() = {}", s.len(), s.chars().count());
    for (i, ch) in s.char_indices().take(3) {
        println!("  byte {i}: {ch:?} ({} byte(s) in UTF-8)", ch.len_utf8());
    }
    println!("'é' is encoded as {:x?}", "é".as_bytes());

    let hello: &str = &s[0..6]; // "héllo": h(1) é(2) l(1) l(1) o(1) = 6 bytes
    println!("&s[0..6] = {hello:?}; is_char_boundary(2) = {}", s.is_char_boundary(2));

    let owned: String = hello.to_owned(); // an independent copy on the heap
    println!("owned = {owned:?}, capacity {}", owned.capacity());

    println!("{}", shout(&s)); // &String coerces to &str (deref coercion)
    println!("{}", shout("literal")); // a &'static str baked into the binary
    println!("{}", shout(&s[7..])); // a sub-slice: no copy
}
