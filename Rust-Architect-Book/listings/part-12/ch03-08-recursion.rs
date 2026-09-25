// verify: debug error:E0733
//! A recursive async fn would contain itself: its future type would have infinite size.
async fn depth(n: u32) -> u32 {
    if n == 0 { 0 } else { 1 + depth(n - 1).await }
}

fn main() {
    println!("{}", futures::executor::block_on(depth(3)));
}
