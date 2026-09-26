// verify: debug ok
// verify: debug miri-ok
// At a boundary, an enum that ARRIVES from C is an integer until Rust has checked it.
// A Rust enum with an undefined tag is an invalid value (Chapter 15.1), so validate first.
#![allow(dead_code)]
use std::mem::size_of;

/// The wire/ABI encoding: a plain u32 that any C or Java caller can produce.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DecisionCode(pub u32);

impl DecisionCode {
    pub const ALLOW: Self = Self(0);
    pub const REVIEW: Self = Self(1);
    pub const BLOCK: Self = Self(2);
}

/// What crosses the boundary: every field has a defined layout and every bit pattern is valid.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MeridianDecision {
    pub code: DecisionCode, // offset 0
    pub risk: u8,           // offset 4: meaningful for REVIEW
    pub rule_id: u64,       // offset 8: meaningful for BLOCK
}

/// What Rust code works with: an ordinary enum, never exposed in a signature.
#[derive(Debug, PartialEq)]
pub enum Decision {
    Allow,
    Review { risk: u8 },
    Block { rule_id: u64 },
}

#[derive(Debug, PartialEq)]
pub struct UnknownDecisionCode(pub u32);

impl From<&Decision> for MeridianDecision {
    fn from(d: &Decision) -> Self {
        match *d {
            Decision::Allow => MeridianDecision { code: DecisionCode::ALLOW, risk: 0, rule_id: 0 },
            Decision::Review { risk } => MeridianDecision { code: DecisionCode::REVIEW, risk, rule_id: 0 },
            Decision::Block { rule_id } => MeridianDecision { code: DecisionCode::BLOCK, risk: 0, rule_id },
        }
    }
}

impl TryFrom<&MeridianDecision> for Decision {
    type Error = UnknownDecisionCode;
    fn try_from(m: &MeridianDecision) -> Result<Self, Self::Error> {
        match m.code {
            DecisionCode::ALLOW => Ok(Decision::Allow),
            DecisionCode::REVIEW => Ok(Decision::Review { risk: m.risk }),
            DecisionCode::BLOCK => Ok(Decision::Block { rule_id: m.rule_id }),
            DecisionCode(other) => Err(UnknownDecisionCode(other)),
        }
    }
}

fn main() {
    println!("size_of::<DecisionCode>() = {}, size_of::<MeridianDecision>() = {}",
        size_of::<DecisionCode>(), size_of::<MeridianDecision>());
    let out = MeridianDecision::from(&Decision::Review { risk: 70 });
    println!("outgoing: {out:?}");
    println!("round trip: {:?}", Decision::try_from(&out));
    // A newer Java client sends code 3 ("CHALLENGE"), which this library version doesn't know:
    let incoming = MeridianDecision { code: DecisionCode(3), risk: 0, rule_id: 0 };
    println!("incoming code 3: {:?}", Decision::try_from(&incoming));
}
