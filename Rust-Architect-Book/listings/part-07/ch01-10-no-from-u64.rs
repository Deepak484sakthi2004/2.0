// verify: debug error:E0277
/// The std-only version: bound on `Into<f64>`, the LOSSLESS conversion trait.
fn mean<T: Copy + Into<f64>>(xs: &[T]) -> f64 {
    xs.iter().map(|&x| x.into()).sum::<f64>() / xs.len() as f64
}

fn main() {
    println!("{}", mean(&[1u32, 2, 3])); // fine: From<u32> for f64 exists
    println!("{}", mean(&[1u64, 2, 3])); // rejected: std has no From<u64> for f64, because it can lose precision
}
