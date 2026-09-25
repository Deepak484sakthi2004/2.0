// verify: debug ok
// Things Java generic code cannot do with T, because T is erased: here T is fully known in every copy.
use std::any::{type_name, TypeId};
use std::mem::{align_of, size_of};

/// A trait with an associated constant and a constructor: "static members" of T.
trait Currency: Copy + Default + std::fmt::Debug {
    const CODE: &'static str;
    const MINOR_UNITS: u32;
    fn from_minor(units: i64) -> Self;
}

#[derive(Clone, Copy, Default, Debug)]
struct Eur(i64);
#[derive(Clone, Copy, Default, Debug)]
struct Jpy(i64);

impl Currency for Eur {
    const CODE: &'static str = "EUR";
    const MINOR_UNITS: u32 = 2;
    fn from_minor(units: i64) -> Self {
        Eur(units)
    }
}
impl Currency for Jpy {
    const CODE: &'static str = "JPY";
    const MINOR_UNITS: u32 = 0;
    fn from_minor(units: i64) -> Self {
        Jpy(units)
    }
}

/// Java: `new T[n]` and `new T()` are illegal, and `T.CODE` does not exist. Rust: all fine.
fn ledger<C: Currency>(n: usize) -> Vec<C> {
    let mut rows = vec![C::default(); n]; // an array of T, filled with T's default
    rows[0] = C::from_minor(12_345); // a "constructor" called through the type parameter
    println!("{} ledger: {} rows, {} minor units, element = {}", C::CODE, rows.len(), C::MINOR_UNITS, type_name::<C>());
    rows
}

fn main() {
    let eur = ledger::<Eur>(3);
    let jpy = ledger::<Jpy>(2);
    println!("first rows: {} {} / {} {}", eur[0].0, Eur::CODE, jpy[0].0, Jpy::CODE);

    // Java: new ArrayList<String>().getClass() == new ArrayList<Integer>().getClass() is TRUE.
    // Rust: Vec<String> and Vec<u8> are different types, and the program can tell at run time.
    println!("TypeId Vec<String> == Vec<u8>? {}", TypeId::of::<Vec<String>>() == TypeId::of::<Vec<u8>>());
    println!("TypeId Vec<u8> == Vec<u8>?     {}", TypeId::of::<Vec<u8>>() == TypeId::of::<Vec<u8>>());
    println!(
        "layout: Option<u8> {}B/align {}, Option<u64> {}B/align {}, Option<Box<u64>> {}B",
        size_of::<Option<u8>>(),
        align_of::<Option<u8>>(),
        size_of::<Option<u64>>(),
        align_of::<Option<u64>>(),
        size_of::<Option<Box<u64>>>()
    );
}
