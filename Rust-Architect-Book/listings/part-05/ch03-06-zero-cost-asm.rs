// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target asm -Mode release -CrateType lib
use std::marker::PhantomData;

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct BasisPoints(u16);

#[derive(Clone, Copy)]
pub struct Cents(pub i64);

#[inline(never)]
pub fn discount_raw(price: i64, bps: u16) -> i64 {
    price - price * bps as i64 / 10_000
}

#[inline(never)]
pub fn discount_typed(price: Cents, bps: BasisPoints) -> Cents {
    Cents(price.0 - price.0 * bps.0 as i64 / 10_000)
}

pub struct Id<T> {
    raw: u64,
    _entity: PhantomData<fn() -> T>,
}
pub struct Order;

#[inline(never)]
pub fn shard_raw(id: u64, shards: u64) -> u64 {
    id % shards
}

#[inline(never)]
pub fn shard_typed(id: Id<Order>, shards: u64) -> u64 {
    id.raw % shards
}
