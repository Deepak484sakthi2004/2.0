// verify: debug error:E0502
// A borrow error in one function and a privacy error (E0451) in another. Only E0502 is reported:
// the privacy pass runs after the driver's "stop if anything failed so far" gate.
mod billing {
    pub struct Invoice {
        pub id: u64,
        total_cents: i64, // private
    }
    impl Invoice {
        pub fn total(&self) -> i64 {
            self.total_cents
        }
    }
}

fn fixture() -> billing::Invoice {
    billing::Invoice { id: 1, total_cents: 500 } // E0451: private field in a struct literal
}

fn borrow_error() -> u32 {
    let mut v = vec![1u32];
    let first = &v[0];
    v.push(2); // E0502
    *first
}

fn main() {
    println!("{} {}", fixture().total(), borrow_error());
}
