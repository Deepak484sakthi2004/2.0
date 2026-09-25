// verify: debug ok
mod accounts {
    #[derive(Debug)]
    pub struct Account {
        pub balance_cents: i64, // made `pub` "temporarily" for a data-migration script
        overdraft_limit: i64,
    }

    impl Account {
        pub fn new(overdraft_limit: i64) -> Account {
            Account { balance_cents: 0, overdraft_limit }
        }

        /// Invariant: balance_cents >= -overdraft_limit
        pub fn withdraw(&mut self, cents: i64) -> Result<(), String> {
            if self.balance_cents - cents < -self.overdraft_limit {
                return Err("insufficient funds".to_string());
            }
            self.balance_cents -= cents;
            Ok(())
        }

        pub fn invariant_holds(&self) -> bool {
            self.balance_cents >= -self.overdraft_limit
        }
    }
}

fn main() {
    let mut acct = accounts::Account::new(5_000);
    println!("{:?}", acct.withdraw(10_000)); // rejected by the guarded path...
    acct.balance_cents -= 10_000; // ...and bypassed through the public field
    println!("{acct:?}, invariant holds: {}", acct.invariant_holds());
}
