// verify: debug ok
#[derive(Debug, Clone, Copy)]
enum LedgerEvent {
    Charge(i64),
    Refund(i64),
    Chargeback(i64), // added six months after `balance_delta` was written
}

fn balance_delta(event: LedgerEvent) -> i64 {
    match event {
        LedgerEvent::Charge(cents) => cents,
        LedgerEvent::Refund(cents) => -cents,
        _ => 0, // "other events don't affect the balance": true on the day it was written
    }
}

fn main() {
    let events = [
        LedgerEvent::Charge(10_000),
        LedgerEvent::Refund(2_000),
        LedgerEvent::Charge(2_000),
        LedgerEvent::Chargeback(10_000), // the customer's bank reversed the first charge
    ];
    let balance: i64 = events.iter().map(|&e| balance_delta(e)).sum();
    println!("merchant balance: {balance} cents (should be 0)");
}
