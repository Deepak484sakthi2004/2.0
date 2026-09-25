// verify: debug miri bounds
// Pointer ARITHMETIC out of bounds is UB even if the result is never dereferenced.
fn main() {
    let a = [1u32, 2, 3];
    let p = a.as_ptr();
    let q = unsafe { p.add(10) }; // 40 bytes past the start of a 12-byte allocation
    let back = unsafe { q.sub(10) };
    println!("{}", unsafe { *back });
}
