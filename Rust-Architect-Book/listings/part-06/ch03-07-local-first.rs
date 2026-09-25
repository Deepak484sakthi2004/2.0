// verify: debug ok
use std::ops::Add;

struct Cents(i64);

// Foreign trait, generic parameter T in the trait's arguments, but the LOCAL type comes first (T0 = Cents): allowed.
impl<T: Into<i64>> Add<T> for Cents {
    type Output = Cents;
    fn add(self, rhs: T) -> Cents {
        Cents(self.0 + rhs.into())
    }
}

struct Ledger(Vec<i64>);

// RFC 2451 (Rust 1.41): a type parameter COVERED by a foreign type (Vec<T>) may precede the local type (Ledger).
impl<T: From<i64>> From<Ledger> for Vec<T> {
    fn from(l: Ledger) -> Vec<T> {
        l.0.into_iter().map(T::from).collect()
    }
}

fn main() {
    let total = Cents(12_550) + 450i32 + 5u8;
    println!("{}", total.0);
    let wide: Vec<i128> = Ledger(vec![1, -2, 3]).into();
    println!("{wide:?}");
}
