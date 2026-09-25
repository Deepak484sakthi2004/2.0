// verify: debug ok
// verify: release ok
// Rust does not promise that distinct functions have distinct addresses. The same program answers
// "same address?" differently in debug (false) and release (true, after function merging).
pub trait Method {
    const BPS: u64;
}
pub struct Card;
pub struct Wallet;
impl Method for Card {
    const BPS: u64 = 290;
}
impl Method for Wallet {
    const BPS: u64 = 290;
}

#[inline(never)]
pub fn fee<M: Method>(amount: u64) -> u64 {
    amount * M::BPS / 10_000
}

fn main() {
    let card: fn(u64) -> u64 = fee::<Card>;
    let wallet: fn(u64) -> u64 = fee::<Wallet>;
    println!("fee(10_000): card={} wallet={}", card(10_000), wallet(10_000));
    println!("same address? {}", std::ptr::fn_addr_eq(card, wallet));
}
