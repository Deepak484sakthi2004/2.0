// verify: debug ok
// try_fold: a fold that can stop early. The accumulator is threaded through; Break/Err ends the loop.
use std::ops::ControlFlow;

#[derive(Debug)]
struct Payout {
    merchant: &'static str,
    cents: u64,
}

fn main() {
    let queue = [
        Payout { merchant: "m-17", cents: 40_000 },
        Payout { merchant: "m-03", cents: 35_000 },
        Payout { merchant: "m-44", cents: 30_000 },
        Payout { merchant: "m-09", cents: 10_000 },
    ];
    let daily_budget = 100_000_u64;

    // Release payouts in order until the next one would exceed the budget.
    let mut released = Vec::new();
    let outcome = queue.iter().try_fold(0_u64, |spent, p| {
        let next = spent + p.cents;
        if next > daily_budget {
            ControlFlow::Break((spent, p.merchant)) // stop: report where we stopped
        } else {
            released.push(p.merchant);
            ControlFlow::Continue(next)
        }
    });
    println!("released {released:?}");
    println!("outcome  {outcome:?}");

    // With Result: checked arithmetic that fails loudly instead of wrapping.
    let amounts = [u64::MAX - 5, 3, 4];
    let total: Result<u64, String> = amounts
        .iter()
        .try_fold(0_u64, |acc, &x| acc.checked_add(x).ok_or(format!("overflow adding {x} to {acc}")));
    println!("checked total: {total:?}");
}
