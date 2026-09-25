// verify: debug error:E0080
// Constant evaluation interprets MIR with the same engine Miri is built on (rustc_const_eval), so it
// checks validity invariants: a `bool` whose byte is 2 is rejected at compile time, not at run time.
const FLAG_BYTE: u8 = 2; // e.g. a byte copied from a wire format

// SAFETY (deliberately violated): `bool` must be 0 or 1; the compiler's evaluator catches this.
const FLAG: bool = unsafe { std::mem::transmute::<u8, bool>(FLAG_BYTE) };

fn main() {
    println!("{FLAG}");
}
