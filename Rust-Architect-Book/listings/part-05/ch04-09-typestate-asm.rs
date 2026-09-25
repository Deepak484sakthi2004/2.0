// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target asm -Mode release -CrateType lib
use std::marker::PhantomData;

pub enum Pending {}
pub enum Authorized {}
pub enum Captured {}

pub struct Payment<S> {
    pub id: u64,
    pub cents: i64,
    _state: PhantomData<S>,
}

impl Payment<Pending> {
    pub fn authorize(self) -> Payment<Authorized> {
        Payment { id: self.id, cents: self.cents, _state: PhantomData }
    }
}
impl Payment<Authorized> {
    pub fn capture(self) -> Payment<Captured> {
        Payment { id: self.id, cents: self.cents, _state: PhantomData }
    }
}

/// Two type-state transitions, then read the amount.
#[inline(never)]
pub fn settle_typed(p: Payment<Pending>) -> i64 {
    p.authorize().capture().cents
}

/// The Chapter 2.5 style: a run-time state field that must be checked.
#[derive(Clone, Copy, PartialEq)]
pub enum State {
    Pending,
    Authorized,
    Captured,
}
pub struct DynPayment {
    pub id: u64,
    pub cents: i64,
    pub state: State,
}

#[inline(never)]
pub fn settle_dynamic(mut p: DynPayment) -> Result<i64, &'static str> {
    if p.state != State::Pending {
        return Err("not pending");
    }
    p.state = State::Authorized;
    if p.state != State::Authorized {
        return Err("not authorized");
    }
    p.state = State::Captured;
    Ok(p.cents)
}

#[inline(never)]
pub fn capture_checked(p: &mut DynPayment) -> Result<i64, &'static str> {
    if p.state != State::Authorized {
        return Err("invalid transition");
    }
    p.state = State::Captured;
    Ok(p.cents)
}
