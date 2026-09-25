// verify: debug ok
use std::collections::HashSet;

/// A home-grown "numeric" trait: convenient, generic... and silently lossy for u64.
trait ToF64 {
    fn to_f64(self) -> f64;
}
impl ToF64 for u32 {
    fn to_f64(self) -> f64 {
        self as f64 // exact: every u32 fits in f64's 53-bit mantissa
    }
}
impl ToF64 for u64 {
    fn to_f64(self) -> f64 {
        self as f64 // NOT exact above 2^53
    }
}

/// Generic de-duplication keyed on the f64 representation (e.g. to feed a stats library).
fn distinct_count<T: ToF64 + Copy>(ids: &[T]) -> usize {
    ids.iter().map(|&id| id.to_f64().to_bits()).collect::<HashSet<u64>>().len()
}

fn main() {
    let small: [u32; 3] = [1, 2, 3];
    let user_ids: [u64; 3] = [9_007_199_254_740_992, 9_007_199_254_740_993, 9_007_199_254_740_994];
    println!("distinct u32 ids: {} of {}", distinct_count(&small), small.len());
    println!("distinct u64 ids: {} of {}", distinct_count(&user_ids), user_ids.len());
    println!("2^53 + 1 as f64 = {}", 9_007_199_254_740_993u64 as f64);
}
