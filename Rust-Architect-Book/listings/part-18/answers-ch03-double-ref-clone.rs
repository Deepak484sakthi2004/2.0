// verify: debug error:suspicious_double_ref_op
// Answer-key check for Chapter 18.3's debugging exercise, Q4: once the return type is Vec<&'a Quote>
// (it needs a named lifetime: elision can't choose between the two input lifetimes), the program
// type-checks and the lint that was hidden behind the E0277 appears (denied here so the check can
// assert it): `q.clone()` on a `&&Quote` clones the reference, not the Quote.
#![deny(suspicious_double_ref_op)]

#[derive(Debug)]
struct Quote {
    symbol: String,
    px: u64,
}

fn snapshot<'a>(book: &[&'a Quote]) -> Vec<&'a Quote> {
    book.iter().map(|q| q.clone()).collect()
}

fn main() {
    let q = Quote { symbol: "MRDN".to_string(), px: 12550 };
    let s = snapshot(&[&q]);
    println!("{} {} {}", s.len(), s[0].symbol, s[0].px);
}
