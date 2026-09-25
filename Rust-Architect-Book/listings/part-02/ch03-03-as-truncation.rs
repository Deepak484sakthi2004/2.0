// verify: debug ok
fn main() {
    let payload = vec![0u8; 70_000];

    // BUG: `as` silently truncates: 70_000 mod 65_536 = 4_464.
    let len_field = payload.len() as u16;
    println!("length field written: {len_field}");

    // FIX: make the narrowing explicit and fallible.
    match u16::try_from(payload.len()) {
        Ok(len) => println!("length field: {len}"),
        Err(e) => println!("rejecting frame: {} bytes does not fit in u16 ({e})", payload.len()),
    }
}
