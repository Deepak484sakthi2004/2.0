// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target mir -Mode debug -CrateType lib
pub enum LedgerEvent {
    Charge { cents: i64 },
    Refund { cents: i64 },
    Chargeback { cents: i64, reason_code: u16 },
}

#[inline(never)]
pub fn balance_delta(e: &LedgerEvent) -> i64 {
    match e {
        LedgerEvent::Charge { cents } => *cents,
        LedgerEvent::Refund { cents } => -*cents,
        LedgerEvent::Chargeback { cents, .. } => -*cents,
    }
}
