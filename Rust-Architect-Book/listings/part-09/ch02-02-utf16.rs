// verify: debug ok
fn main() {
    let s = "héllo 😀";
    let utf16: Vec<u16> = s.encode_utf16().collect();
    println!("{s:?}: {} UTF-8 bytes, {} chars, {} UTF-16 code units", s.len(), s.chars().count(), utf16.len());
    println!("UTF-16 units: {:04X?}", utf16);

    // Round trip: valid UTF-16 comes back exactly.
    let back = String::from_utf16(&utf16).unwrap();
    assert_eq!(back, s);

    // A lone surrogate: legal in a Java String / Windows filename, illegal in a Rust String.
    let broken: Vec<u16> = vec![0x0068, 0xD83D, 0x0069]; // 'h', high surrogate alone, 'i'
    match String::from_utf16(&broken) {
        Ok(s) => println!("unexpected: {s}"),
        Err(e) => println!("from_utf16(lone surrogate): Err({e})"),
    }
    println!("from_utf16_lossy:           {:?}", String::from_utf16_lossy(&broken));

    // Bytes from the network: lossy decoding borrows when it can, allocates when it must.
    let good = b"price=42";
    let bad = b"price=\xFF42";
    for bytes in [&good[..], &bad[..]] {
        let cow = String::from_utf8_lossy(bytes);
        let kind = match &cow {
            std::borrow::Cow::Borrowed(_) => "Borrowed (no allocation)",
            std::borrow::Cow::Owned(_) => "Owned (allocated, U+FFFD inserted)",
        };
        println!("from_utf8_lossy({bytes:?}) = {cow:?} -> {kind}");
    }

    // Java's String.length() counts UTF-16 units; Rust's len() counts UTF-8 bytes.
    let name = "Zoë🚀";
    println!(
        "{name:?}: Rust len() = {}, Java length() would be {}, code points = {}",
        name.len(),
        name.encode_utf16().count(),
        name.chars().count()
    );
}
