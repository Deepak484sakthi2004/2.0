// verify: debug error:E0277
#[derive(Clone, Copy, Debug)]
struct Money {
    cents: i64,
}

fn largest<T: PartialOrd + Copy>(xs: &[T]) -> Option<T> {
    let mut best = *xs.first()?;
    for &x in xs {
        if x > best {
            best = x;
        }
    }
    Some(best)
}

fn main() {
    let payments = [Money { cents: 500 }, Money { cents: 1200 }];
    println!("{:?}", largest(&payments)); // Money never said it can be compared
}
