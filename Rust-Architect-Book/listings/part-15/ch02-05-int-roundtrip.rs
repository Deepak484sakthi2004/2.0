// verify: debug miri-ok
// pointer -> usize -> pointer: legal ("exposed" provenance), but Miri warns that it may miss bugs.
fn main() {
    let x = Box::new(41u64);
    let addr = &*x as *const u64 as usize; // pointer -> integer (exposes provenance)
    let p = addr as *const u64; // integer -> pointer (picks up some exposed provenance)
    println!("{}", unsafe { *p } + 1);
}
