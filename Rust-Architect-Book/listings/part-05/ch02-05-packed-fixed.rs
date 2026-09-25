// verify: debug ok
// verify: debug miri-ok
use std::mem::size_of;

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct WireHeader {
    kind: u8,
    length: u32, // at offset 1: NOT 4-byte aligned
}

fn main() {
    let h = WireHeader { kind: 1, length: 512 };
    // Copy the field out by value (the compiler emits an unaligned load); never take a reference.
    let length = h.length;
    let kind = h.kind;
    // For a pointer, read_unaligned is the explicit tool.
    let p = std::ptr::addr_of!(h.length);
    // SAFETY: `p` points to a live, initialized u32 inside `h`; read_unaligned has no alignment requirement.
    let via_ptr = unsafe { p.read_unaligned() };
    println!("size_of::<WireHeader>() = {}, kind={kind}, length={length}, via_ptr={via_ptr}", size_of::<WireHeader>());
}
