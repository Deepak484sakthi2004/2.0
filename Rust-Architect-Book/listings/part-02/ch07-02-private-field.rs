// verify: debug error:E0616
mod billing {
    pub struct Invoice {
        pub customer: String,
        total_cents: i64,
    }

    impl Invoice {
        pub fn new(customer: &str) -> Invoice {
            Invoice { customer: customer.to_string(), total_cents: 0 }
        }
    }
}

fn main() {
    let inv = billing::Invoice::new("acme");
    println!("{} owes {}", inv.customer, inv.total_cents);
}
