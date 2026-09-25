// verify: debug error:E0451
// ch01-05 with the borrow error removed: now the privacy pass runs and reports E0451.
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

fn main() {
    println!("{}", fixture().total());
}
