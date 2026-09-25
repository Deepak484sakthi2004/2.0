// verify: debug error:E0133
// Unsafety checking runs on THIR (the typed tree between HIR and MIR): calling an `unsafe fn`
// outside an `unsafe` block is rejected there.
unsafe fn read_raw(p: *const u64) -> u64 {
    // SAFETY: callers must pass a valid, aligned, initialized pointer.
    unsafe { *p }
}

fn main() {
    let x = 7u64;
    let v = read_raw(&x); // E0133
    println!("{v}");
}
