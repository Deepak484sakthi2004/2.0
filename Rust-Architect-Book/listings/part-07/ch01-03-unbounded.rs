// verify: debug error:E0369
// The body uses `>`, but the signature never promised that T supports it.
fn largest<T: Copy>(xs: &[T]) -> Option<T> {
    let mut best = *xs.first()?;
    for &x in xs {
        if x > best {
            best = x;
        }
    }
    Some(best)
}

fn main() {
    println!("{:?}", largest(&[3, 9, 4]));
}
