// verify: release ok
// Two different functions with identical bodies, compared with `==`: the
// unpredictable_function_pointer_comparisons lint warns (warn by default on 1.98.1), and in release
// the comparison prints `true` because LLVM merged the functions.
fn a(x: u64) -> u64 {
    x * 2
}

fn b(x: u64) -> u64 {
    x * 2
}

fn main() {
    let f: fn(u64) -> u64 = a;
    let g: fn(u64) -> u64 = b;
    println!("{}", f == g);
}
