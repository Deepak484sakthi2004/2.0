// verify: debug error:E0728
//! Function coloring: a synchronous function can't `.await`. To call async code it must either
//! become async itself (and so must its callers) or block on an executor.
async fn load_limits(merchant: u64) -> u64 {
    merchant * 1_000
}

fn check_limit(merchant: u64, amount: u64) -> bool {
    amount <= load_limits(merchant).await
}

fn main() {
    println!("{}", check_limit(7, 500));
}
