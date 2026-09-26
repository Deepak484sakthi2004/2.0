// verify: debug error:chained
// Listing 17.3-3: Rust's grammar makes comparison operators non-associative. The parser rejects
// `a < b < c` (and suggests the fix) instead of choosing an associativity.
fn main() {
    let (a, b, c) = (1, 2, 3);
    if a < b < c {
        println!("ascending");
    }
}
