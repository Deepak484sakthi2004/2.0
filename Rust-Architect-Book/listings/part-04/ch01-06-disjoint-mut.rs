// verify: debug ok
/// Disjointness proven by a SAFE API that checks at run time (and contains the unsafe inside std).
fn transfer(balances: &mut [i64], from: usize, to: usize, amount: i64) -> Result<(), String> {
    let [a, b] = balances
        .get_disjoint_mut([from, to])
        .map_err(|e| format!("bad accounts {from}->{to}: {e:?}"))?;
    *a -= amount;
    *b += amount;
    Ok(())
}

/// Disjointness proven by CONSTRUCTION: split the slice into two non-overlapping halves.
fn swap_halves(xs: &mut [u32]) {
    let mid = xs.len() / 2;
    let (left, right) = xs.split_at_mut(mid);
    for (l, r) in left.iter_mut().zip(right.iter_mut()) {
        std::mem::swap(l, r);
    }
}

fn main() {
    let mut balances = vec![100, 50, 10];
    println!("{:?} -> {balances:?}", transfer(&mut balances, 0, 1, 30));
    println!("{:?}", transfer(&mut balances, 2, 2, 5));
    println!("{:?}", transfer(&mut balances, 0, 9, 5));

    let mut xs = [1, 2, 3, 4];
    swap_halves(&mut xs);
    println!("{xs:?}");
}
