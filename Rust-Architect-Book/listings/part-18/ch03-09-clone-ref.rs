// verify: debug error:E0277
// Chapter 18.3 debugging exercise. `q` is `&&Quote` and `Quote` isn't `Clone`, so method lookup finds
// `Clone for &Quote` at the first autoderef step: `q.clone()` copies the REFERENCE.
#[derive(Debug)]
struct Quote {
    symbol: String,
    px: u64,
}

fn snapshot(book: &[&Quote]) -> Vec<Quote> {
    book.iter().map(|q| q.clone()).collect()
}

fn main() {
    let a = Quote { symbol: "MRDN".into(), px: 12_550 };
    let snap = snapshot(&[&a]);
    println!("{snap:?}");
}
