// verify: release ok
use std::sync::{Barrier, Mutex};
use std::thread;
use std::time::Duration;

struct Account {
    balance: Mutex<i64>,
}

impl Account {
    // BUG: a race condition, not a data race. Every access is synchronized,
    // but the check and the act happen under two separate lock acquisitions.
    fn withdraw_racy(&self, amount: i64) -> bool {
        let current = *self.balance.lock().unwrap(); // lock, read, unlock
        if current >= amount {
            thread::sleep(Duration::from_millis(1)); // "real work" between check and act
            *self.balance.lock().unwrap() -= amount; // lock again: the world may have changed
            true
        } else {
            false
        }
    }

    // Correct: check and act under one guard.
    fn withdraw(&self, amount: i64) -> bool {
        let mut balance = self.balance.lock().unwrap();
        if *balance >= amount {
            *balance -= amount;
            true
        } else {
            false
        }
    }
}

// Eight threads each try to withdraw 100 from an account holding 100.
fn final_balance(withdraw: fn(&Account, i64) -> bool) -> i64 {
    let account = Account { balance: Mutex::new(100) };
    let start = Barrier::new(8);
    thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| {
                start.wait(); // line the threads up so they really race
                withdraw(&account, 100)
            });
        }
    });
    account.balance.into_inner().unwrap()
}

fn main() {
    println!("racy:    final balance = {}", final_balance(Account::withdraw_racy));
    println!("correct: final balance = {}", final_balance(Account::withdraw));
}
