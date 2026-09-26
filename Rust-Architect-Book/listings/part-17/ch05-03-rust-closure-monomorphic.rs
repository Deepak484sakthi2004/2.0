// verify: debug error:E0308
// Listing 17.5-3: Rust's inference is Hindley-Milner-like inside a function body, but a closure is
// NOT generalized: its parameter type is fixed by the first use. (HM's let-polymorphism would
// accept this program; Rust asks you to write a generic fn instead.)
fn main() {
    let id = |x| x;
    let a = id(1);
    let b = id(true);
    println!("{a} {b}");
}
