// verify: debug error:E0597
fn largest<T: PartialOrd>(items: &[T]) -> Option<&T> {
    let mut best = items.first()?;
    for item in items {
        if item > best {
            best = item;
        }
    }
    Some(best)
}

fn main() {
    let top;
    {
        let scores = vec![3, 9, 4];
        top = largest(&scores);
    } // `scores` is dropped here
    println!("{top:?}");
}
