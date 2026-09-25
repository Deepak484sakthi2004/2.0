// verify: debug error:E0793
#[repr(C, packed)]
struct WireHeader {
    kind: u8,
    length: u32, // at offset 1: NOT 4-byte aligned
}

fn main() {
    let h = WireHeader { kind: 1, length: 512 };
    let len_ref: &u32 = &h.length; // a &u32 must be aligned; this one can't be
    println!("{}", len_ref);
}
