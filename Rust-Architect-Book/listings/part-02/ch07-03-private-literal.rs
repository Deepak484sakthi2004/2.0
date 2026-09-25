// verify: debug error:E0451
mod billing {
    pub struct Invoice {
        pub customer: String,
        total_cents: i64,
    }
}

fn main() {
    // Constructing a struct literal requires access to EVERY field.
    let forged = billing::Invoice { customer: "x".into(), total_cents: -1 };
    println!("{}", forged.customer);
}
