// verify: debug error:E0382
fn main() {
    let amounts = vec![1_200_u64, 550, 99];
    let big = amounts.iter().filter(|&&a| a > 100);
    let count = big.count(); // count() takes the iterator by value: consumed
    let total: u64 = big.sum(); // Java: IllegalStateException at run time; Rust: rejected at compile time
    println!("{count} {total}");
}
