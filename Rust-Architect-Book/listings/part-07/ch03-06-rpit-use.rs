// verify: debug ok
/// `use<T>` lists exactly what the opaque type may capture: T, but not the borrow's lifetime.
fn indices<T>(v: &Vec<T>) -> impl Iterator<Item = usize> + use<T> {
    0..v.len()
}

/// The opposite case: the hidden type really does borrow `v`, so it must capture its lifetime.
fn positive<'a>(v: &'a [i64]) -> impl Iterator<Item = i64> + use<'a> {
    v.iter().copied().filter(|&x| x > 0)
}

fn main() {
    let mut data = vec![10, 20, 30];
    let idx = indices(&data);
    data.push(40); // fine: the signature promises `idx` holds no borrow
    println!("{} indices, {} items", idx.count(), data.len());

    let deltas = [-5, 7, 0, 12];
    let pos: Vec<i64> = positive(&deltas).collect();
    println!("positive deltas: {pos:?}");
}
