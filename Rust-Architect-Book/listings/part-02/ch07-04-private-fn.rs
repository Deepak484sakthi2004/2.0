// verify: debug error:E0603
mod ledger {
    fn post_entry(cents: i64) -> i64 {
        cents
    }

    pub fn transfer(cents: i64) -> i64 {
        post_entry(-cents) + post_entry(cents)
    }
}

fn main() {
    println!("{}", ledger::transfer(100));
    println!("{}", ledger::post_entry(5));
}
