// verify: debug ok
struct Account {
    balance: i64,
}

struct Bank {
    accounts: Vec<Account>,
    rate_bps: i64,
}

impl Bank {
    fn apply_interest(&mut self) {
        // Fix 1: borrow DISJOINT FIELDS directly: `self.accounts` mutably, `self.rate_bps` by copy.
        for acct in self.accounts.iter_mut() {
            acct.balance += acct.balance * self.rate_bps / 10_000;
        }
    }

    fn apply_interest_v2(&mut self) {
        // Fix 2: read what you need BEFORE taking the mutable borrow.
        let rate = self.rate_bps;
        for acct in &mut self.accounts {
            acct.balance += acct.balance * rate / 10_000;
        }
    }
}

fn main() {
    let mut bank = Bank { accounts: vec![Account { balance: 10_000 }], rate_bps: 250 };
    bank.apply_interest();
    bank.apply_interest_v2();
    println!("{}", bank.accounts[0].balance);
}
