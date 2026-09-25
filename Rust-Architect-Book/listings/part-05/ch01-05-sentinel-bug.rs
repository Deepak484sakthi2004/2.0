// verify: debug ok
use std::num::NonZeroU32;

// BEFORE: a sentinel. `0` means "unlimited" -- by convention, in a comment, somewhere.
fn allowed_sentinel(requests_this_second: u32, limit_per_second: u32) -> bool {
    limit_per_second == 0 || requests_this_second < limit_per_second
}

// AFTER: the two meanings are two variants; a zero limit is not representable.
#[derive(Debug, Clone, Copy)]
enum Limit {
    Unlimited,
    PerSecond(NonZeroU32),
}

fn allowed(requests_this_second: u32, limit: Limit) -> bool {
    match limit {
        Limit::Unlimited => true,
        Limit::PerSecond(n) => requests_this_second < n.get(),
    }
}

fn main() {
    // An operator sets a tenant's limit to 0 intending "block this tenant entirely"...
    let blocked_tenant_limit = 0;
    println!("sentinel: request #1000 allowed? {}", allowed_sentinel(1000, blocked_tenant_limit));

    // With the enum, "0 per second" must be written down as a decision:
    println!("enum: 0 as a limit is {:?}", NonZeroU32::new(0));
    println!("enum: unlimited allows #1000? {}", allowed(1000, Limit::Unlimited));
    let ten = Limit::PerSecond(NonZeroU32::new(10).unwrap());
    println!("enum: 10/s allows #9? {}  #10? {}", allowed(9, ten), allowed(10, ten));
    println!("size_of::<Limit>() = {}", std::mem::size_of::<Limit>());
}
