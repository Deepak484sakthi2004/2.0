// verify: debug ok
mod billing {
    // Private by default: visible inside `billing` and its descendants only.
    const MAX_DISCOUNT_BPS: u32 = 5_000;

    pub mod money {
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct Cents(i64); // public type, PRIVATE field: the only way in is a constructor

        impl Cents {
            pub fn from_i64(v: i64) -> Option<Cents> {
                if v >= 0 { Some(Cents(v)) } else { None }
            }
            pub fn get(self) -> i64 {
                self.0
            }
            // Visible only inside `crate::billing`: trusted code may skip the check.
            pub(in crate::billing) fn raw(v: i64) -> Cents {
                Cents(v)
            }
        }
    }

    pub struct Invoice {
        pub customer: String, // public: plain data
        total: money::Cents,  // private: guarded by methods
        discount_bps: u32,
    }

    impl Invoice {
        pub fn new(customer: &str) -> Invoice {
            Invoice { customer: customer.to_string(), total: money::Cents::raw(0), discount_bps: 0 }
        }
        pub fn add_line(&mut self, amount: money::Cents) {
            self.total = money::Cents::raw(self.total.get() + amount.get());
        }
        pub fn apply_discount(&mut self, bps: u32) -> Result<(), String> {
            if bps > MAX_DISCOUNT_BPS {
                // a child module reading its parent's private item: allowed
                return Err(format!("discount {bps} bps exceeds cap"));
            }
            self.discount_bps = bps;
            Ok(())
        }
        pub fn due(&self) -> money::Cents {
            let t = self.total.get();
            money::Cents::raw(t - t * self.discount_bps as i64 / 10_000)
        }
    }

    // Visible anywhere in this crate, never outside it.
    pub(crate) fn audit_line(inv: &Invoice) -> String {
        format!("{}: due {} cents", inv.customer, inv.due().get())
    }
}

// A facade: re-export what callers should use; hide the internal module layout.
pub use billing::money::Cents;
pub use billing::Invoice;

fn main() {
    let mut inv = Invoice::new("acme");
    inv.add_line(Cents::from_i64(12_000).unwrap());
    inv.add_line(Cents::from_i64(3_000).unwrap());
    println!("{:?}", inv.apply_discount(9_000));
    inv.apply_discount(1_000).unwrap();
    println!("{}", billing::audit_line(&inv));
    println!("negative amount: {:?}", Cents::from_i64(-5));
}
