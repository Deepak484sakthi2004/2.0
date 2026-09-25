// verify: release build
// Source for the release asm artifact in Chapter 18.6: two monomorphized instances that happen to
// compile to identical machine code, which LLVM merges into one body plus an alias.
pub trait Method {
    const BPS: u64;
}
pub struct Card;
pub struct Wallet;
impl Method for Card {
    const BPS: u64 = 290;
}
impl Method for Wallet {
    const BPS: u64 = 290; // same rate as cards today, so the two instances compile to identical code
}

#[inline(never)]
pub fn fee<M: Method>(amount: u64) -> u64 {
    amount * M::BPS / 10_000
}

pub fn fee_card(amount: u64) -> u64 {
    fee::<Card>(amount)
}

pub fn fee_wallet(amount: u64) -> u64 {
    fee::<Wallet>(amount)
}
