// verify: debug error:unused_must_use
//! Forgetting `.await` compiles by default (with a warning). Many teams deny the lint.
#![deny(unused_must_use)]

async fn charge(amount: u64) -> u64 {
    println!("charging {amount}");
    amount
}

async fn checkout() {
    charge(500); // BUG: builds a future and drops it; nothing is charged
    println!("done");
}

fn main() {
    futures::executor::block_on(checkout());
}
