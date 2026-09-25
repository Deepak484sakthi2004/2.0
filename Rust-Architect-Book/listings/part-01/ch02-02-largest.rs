// verify: debug ok
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
    let scores = vec![3, 9, 4];
    println!("{:?}", largest(&scores));
    let empty: Vec<i32> = Vec::new();
    println!("{:?}", largest(&empty));
    let words = ["kiwi", "apple", "mango"];
    println!("{:?}", largest(&words));
}
