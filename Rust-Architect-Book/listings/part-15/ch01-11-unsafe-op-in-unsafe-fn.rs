// verify: debug ok
// verify: debug@2021 ok
/// # Safety
/// `p` must be non-null, aligned, and point to an initialized `u64` that no one else is writing.
unsafe fn read_counter(p: *const u64) -> u64 {
    *p // edition 2024: warning, the body of an `unsafe fn` is no longer an implicit unsafe block
}

fn main() {
    let x = 42u64;
    // SAFETY: `&x` is non-null, aligned, initialized, and not written concurrently.
    let v = unsafe { read_counter(&x) };
    println!("{v}");
}
