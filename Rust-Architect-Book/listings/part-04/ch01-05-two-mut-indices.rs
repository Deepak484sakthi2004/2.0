// verify: debug error:E0499
fn transfer(balances: &mut [i64], from: usize, to: usize, amount: i64) {
    let a = &mut balances[from];
    let b = &mut balances[to]; // even when from != to, the checker can't prove disjointness
    *a -= amount;
    *b += amount;
}

fn main() {
    let mut balances = vec![100, 50];
    transfer(&mut balances, 0, 1, 30);
    println!("{balances:?}");
}
