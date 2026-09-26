// verify: debug error:E0381
// Listing 17.6-4: rustc runs the same forward "must" analysis (on MIR, as part of borrow checking)
// and rejects a use on a path where the variable was never assigned.
fn main() {
    let x: i32;
    let c = std::env::args().count() > 5;
    if c {
        x = 1;
    }
    println!("{}", x + 1);
}
