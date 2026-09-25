// verify: debug error:E0369
fn largest<T>(items: &[T]) -> Option<&T> {
    let mut best = items.first()?;
    for item in items {
        if item > best {
            best = item;
        }
    }
    Some(best)
}

fn main() {
    println!("{:?}", largest(&[3, 9, 4]));
}
