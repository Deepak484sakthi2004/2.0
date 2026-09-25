// verify: debug error:E0004
// The ledger's event enum gained a variant. Every `match` without a wildcard now fails to compile.
#[derive(Debug)]
enum LedgerEvent {
    Charge { cents: i64 },
    Refund { cents: i64 },
    Chargeback { cents: i64, reason_code: u16 }, // NEW
}

fn balance_delta(e: &LedgerEvent) -> i64 {
    match e {
        LedgerEvent::Charge { cents } => *cents,
        LedgerEvent::Refund { cents } => -*cents,
    }
}

fn main() {
    let e = LedgerEvent::Chargeback { cents: 4_999, reason_code: 4837 };
    println!("{e:?} -> {}", balance_delta(&e));
}
