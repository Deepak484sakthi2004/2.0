// verify: debug error:E0599
/// A Java habit: "give me a fresh T". In Rust the bound must say T can be constructed.
fn fresh_batch<T>(n: usize) -> Vec<T> {
    (0..n).map(|_| T::new()).collect()
}

fn main() {
    let batch: Vec<String> = fresh_batch(3);
    println!("{}", batch.len());
}
