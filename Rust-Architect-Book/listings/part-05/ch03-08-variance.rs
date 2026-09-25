// verify: debug error:lifetime
use std::marker::PhantomData;

/// Covariant in 'a: a Token<'long> may be used where a Token<'short> is expected.
pub struct Token<'a>(PhantomData<&'a ()>);

/// Invariant in 'a: the lifetime is a *brand* that must match exactly.
pub struct Brand<'a>(PhantomData<fn(&'a ()) -> &'a ()>);

pub fn shorten_token<'short, 'long: 'short>(t: Token<'long>) -> Token<'short> {
    t // fine: covariance
}

pub fn shorten_brand<'short, 'long: 'short>(b: Brand<'long>) -> Brand<'short> {
    b // rejected: invariance
}

fn main() {}
