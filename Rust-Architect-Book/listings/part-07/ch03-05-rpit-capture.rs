// verify: debug error:E0502
// verify: debug@2021 ok
/// Returns the valid indices of `v`. The hidden type is Range<usize>: it borrows nothing.
fn indices<T>(v: &Vec<T>) -> impl Iterator<Item = usize> {
    0..v.len()
}

fn main() {
    let mut data = vec![10, 20, 30];
    let idx = indices(&data);
    data.push(40); // edition 2024: `idx` is assumed to hold the borrow of `data`
    println!("{} indices, {} items", idx.count(), data.len());
}
