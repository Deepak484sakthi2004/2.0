// verify: debug error:E0381
// Listing 17.6-7: definite assignment is a property of CFG paths, not of values. Even a constant
// `true` condition leaves a path on which `x` is never assigned, so rustc rejects the use.
// (Java's definite-assignment rules treat a constant `true` specially and accept the equivalent.)
fn main() {
    let x: i32;
    if true {
        x = 1;
    }
    println!("{x}");
}
