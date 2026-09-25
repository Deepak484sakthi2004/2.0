// verify: debug error:long_running_const_eval
// Const evaluation has no fixed step limit on 1.98.1; a deny-by-default lint stops evaluations that
// run "too long" (usually an accidental infinite loop). Allow it only for a known-long table.
const SUM: u64 = {
    let mut i = 0u64;
    let mut s = 0u64;
    while i < 50_000_000 {
        s = s.wrapping_add(i);
        i += 1;
    }
    s
};

fn main() {
    println!("{SUM}");
}
