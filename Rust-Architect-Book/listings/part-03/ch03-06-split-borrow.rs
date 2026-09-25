// verify: debug error:E0502
struct Account {
    balance: i64,
}

struct Bank {
    accounts: Vec<Account>,
    rate_bps: i64,
}

impl Bank {
    fn rate(&self) -> i64 {
        self.rate_bps
    }

    fn apply_interest(&mut self) {
        for acct in self.accounts.iter_mut() {
            acct.balance += acct.balance * self.rate() / 10_000;
        }
    }
}

fn main() {
    let mut bank = Bank { accounts: vec![Account { balance: 10_000 }], rate_bps: 250 };
    bank.apply_interest();
    println!("{}", bank.accounts[0].balance);
}
