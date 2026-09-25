// verify: debug ok
// Iterators over borrowed data are cheap values: clone the pipeline to traverse it twice.
fn main() {
    let amounts = vec![1_200_u64, 550, 99, 4_000];
    let big = amounts.iter().filter(|&&a| a > 100); // Filter<Iter<u64>, closure>: Clone, because both parts are
    let count = big.clone().count();
    let total: u64 = big.sum();
    println!("count={count} total={total} mean={}", total / count as u64);
    println!("size of the pipeline value: {} B", std::mem::size_of_val(&amounts.iter().filter(|&&a| a > 100)));
}
