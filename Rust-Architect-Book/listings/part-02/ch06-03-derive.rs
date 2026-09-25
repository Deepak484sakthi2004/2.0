// verify: debug ok
// Inspect with: tools\emit.ps1 <this file> -Target expand   (nightly; shows the code #[derive] generates)
#[derive(Debug, Clone, PartialEq)]
pub struct Money {
    cents: i64,
    currency: &'static str,
}

fn main() {
    let a = Money { cents: 500, currency: "EUR" };
    let b = a.clone();
    println!("{a:?} == {b:?}: {}", a == b);
}
