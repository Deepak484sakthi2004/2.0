// verify: debug error:future
//! On the multi-thread runtime a task may resume on another worker thread, so tokio::spawn requires
//! the future to be Send. An Rc held across an .await makes it !Send.
use std::rc::Rc;

async fn fetch_rate() -> u32 {
    tokio::task::yield_now().await;
    42
}

#[tokio::main]
async fn main() {
    let task = tokio::spawn(async {
        let cache = Rc::new(vec![1u32, 2, 3]);
        let rate = fetch_rate().await; // `cache` is alive across this .await
        cache.len() as u32 + rate
    });
    println!("{}", task.await.unwrap());
}
