// verify: debug ok
// verify: debug miri allocation
// Same ADDRESS, different PROVENANCE: a pointer derived from `a` may not access `b`.
fn main() {
    let a = [1u32, 2];
    let b = [3u32, 4];
    let pa = a.as_ptr();
    let pb = b.as_ptr();
    println!("one past a == &b[0]? {}", pa.wrapping_add(2) == pb);
    // Build a pointer with b's address but a's provenance.
    let forged = pa.with_addr(pb.addr());
    println!("forged == pb? {}", forged == pb);
    let v = unsafe { *forged };
    println!("read {v} through a pointer derived from `a`");
}
