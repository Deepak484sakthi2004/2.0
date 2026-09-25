// verify: debug ok
// verify: debug miri-ok
use std::ptr::NonNull;

/// A wire struct: `len` sits at offset 1, so it is never 4-aligned.
#[repr(C, packed)]
struct WireHeader {
    kind: u8,
    len: u32,
}

fn main() {
    let xs = [10u32, 20, 30, 40];
    let base: *const u32 = xs.as_ptr();
    // SAFETY: base + 2 stays inside the 4-element array.
    let third = unsafe { base.add(2) }; // arithmetic in units of T: +8 bytes
    let end = base.wrapping_add(xs.len()); // one past the end: may exist, must not be read
    // SAFETY: `third` points to xs[2], initialized and alive for this whole function.
    println!("*third = {}", unsafe { *third });
    // SAFETY: both pointers are derived from `xs` and lie within (or one past) it.
    println!("end - base = {} elements", unsafe { end.offset_from(base) });

    let h = WireHeader { kind: 7, len: 1_000 };
    // `&h.len` would be a misaligned reference (error E0793). A raw pointer may be misaligned:
    let p: *const u32 = &raw const h.len;
    // SAFETY: `p` points to an initialized u32 inside `h`; read_unaligned has no alignment requirement.
    println!("kind = {}, len = {}", h.kind, unsafe { p.read_unaligned() });

    let nn: NonNull<u32> = NonNull::from(&xs[0]);
    println!(
        "NonNull<u32> = {} B, Option<NonNull<u32>> = {} B, *const u32 = {} B, first = {}",
        size_of::<NonNull<u32>>(),
        size_of::<Option<NonNull<u32>>>(),
        size_of::<*const u32>(),
        // SAFETY: `nn` was made from a live reference to xs[0].
        unsafe { *nn.as_ptr() }
    );
}
