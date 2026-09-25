// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target asm -Mode release
pub trait Fee {
    fn fee_cents(&self, amount_cents: u64) -> u64;
}

pub struct Percent(pub u64); // basis points
impl Fee for Percent {
    fn fee_cents(&self, amount_cents: u64) -> u64 {
        amount_cents * self.0 / 10_000
    }
}

/// Static dispatch: one copy per F; the call to fee_cents is a direct call or inlined.
#[inline(never)]
pub fn total_fees_static<F: Fee>(fee: &F, amounts: &[u64]) -> u64 {
    amounts.iter().map(|&a| fee.fee_cents(a)).sum()
}

/// Dynamic dispatch: one copy for every implementor; each call goes through the vtable.
#[inline(never)]
pub fn total_fees_dyn(fee: &dyn Fee, amounts: &[u64]) -> u64 {
    amounts.iter().map(|&a| fee.fee_cents(a)).sum()
}

pub fn use_static(amounts: &[u64]) -> u64 {
    total_fees_static(&Percent(290), amounts)
}
pub fn use_dyn(fee: &dyn Fee, amounts: &[u64]) -> u64 {
    total_fees_dyn(fee, amounts)
}
