// verify: debug ok
// Which combinations of source and consumer make `by_ref().zip(0..3)` drop an item?
// [LIB] Zip's docs allow one extra pull from the first iterator; std avoids it on some internal-iteration
// paths when both sides report an exact length (TrustedLen). Don't depend on either behavior.
fn main() {
    // (a) exact-length source (Vec), external iteration: a `for` loop calls next()
    let mut it = (1..=10u32).collect::<Vec<_>>().into_iter();
    let mut got = Vec::new();
    for _ in 0..4 {
        let mut b = Vec::new();
        for (e, _) in it.by_ref().zip(0..3) {
            b.push(e);
        }
        got.push(b);
    }
    println!("(a) Vec source,    for loop: {got:?}");

    // (b) exact-length source, internal iteration: collect
    let mut it = (1..=10u32).collect::<Vec<_>>().into_iter();
    let got: Vec<Vec<u32>> = (0..4).map(|_| it.by_ref().zip(0..3).map(|(e, _)| e).collect()).collect();
    println!("(b) Vec source,    collect:  {got:?}");

    // (c) inexact source (a filter in front), internal iteration: collect
    let mut it = (1..=10u32).collect::<Vec<_>>().into_iter().filter(|_| true);
    let got: Vec<Vec<u32>> = (0..4).map(|_| it.by_ref().zip(0..3).map(|(e, _)| e).collect()).collect();
    println!("(c) filter source, collect:  {got:?}");
}
