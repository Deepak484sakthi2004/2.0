// verify: release ok
use std::hint::black_box;
use std::time::Instant;

/// O(n^2): every remove() shifts the whole tail left by one.
fn remove_evens_by_index(v: &mut Vec<u64>) {
    let mut i = 0;
    while i < v.len() {
        if v[i] % 2 == 0 {
            v.remove(i);
        } else {
            i += 1;
        }
    }
}

/// O(n): one pass, compacting survivors toward the front.
fn remove_evens_retain(v: &mut Vec<u64>) {
    v.retain(|x| x % 2 != 0);
}

fn main() {
    let n = 50_000u64;
    let mut a: Vec<u64> = (0..n).collect();
    let mut b = a.clone();

    let t = Instant::now();
    remove_evens_by_index(black_box(&mut a));
    let by_index = t.elapsed();

    let t = Instant::now();
    remove_evens_retain(black_box(&mut b));
    let retain = t.elapsed();

    assert_eq!(a, b);
    println!("n = {n}, survivors = {}", a.len());
    println!("remove() in a loop: {by_index:?}");
    println!("retain():           {retain:?}");
}
