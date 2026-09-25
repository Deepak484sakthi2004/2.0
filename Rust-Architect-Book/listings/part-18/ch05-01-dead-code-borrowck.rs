// verify: debug error:E0499
// Borrow checking runs on MIR BEFORE optimization: code that can never execute is still checked.
fn main() {
    let mut balance = 100;
    if false {
        let a = &mut balance;
        let b = &mut balance; // never executes, still rejected
        *a += 1;
        *b += 1;
    }
    println!("{balance}");
}
