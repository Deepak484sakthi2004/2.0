// verify: debug ok
// size_hint through adapters: exactness is lost by filter/flatten and kept by map/take/skip/zip.
fn show(label: &str, hint: (usize, Option<usize>)) {
    println!("{label:<28} {hint:?}");
}

fn main() {
    let v: Vec<u32> = (1..=100).collect();
    let w = vec![vec![1u32, 2], vec![3]];
    show("v.iter()", v.iter().size_hint());
    show("v.iter().map(..)", v.iter().map(|x| x * 2).size_hint());
    show("v.iter().filter(..)", v.iter().filter(|x| **x % 2 == 0).size_hint());
    show("v.iter().take(10)", v.iter().take(10).size_hint());
    show("v.iter().skip(95)", v.iter().skip(95).size_hint());
    show("v.iter().zip(0..30)", v.iter().zip(0..30).size_hint());
    show("v.iter().chain(v.iter())", v.iter().chain(v.iter()).size_hint());
    show("w.iter().flatten()", w.iter().flatten().size_hint());
    show("(0..).take(5)", (0u32..).take(5).size_hint());
    show("(0..)", (0u32..).size_hint());
    println!("ExactSizeIterator::len of a map: {}", v.iter().map(|x| x + 1).len());
    println!("DoubleEnded: last multiple of 7 at index {:?}", v.iter().rposition(|&x| x % 7 == 0));
}
