// verify: debug error:future
//! Box<dyn Error> (without + Send + Sync) held across an .await: the future is !Send.
//! Chapter 8.2 promised this one.
use std::error::Error;

async fn audit() {}

async fn parse_amount(s: &str) -> Result<u32, Box<dyn Error>> {
    let parsed: Result<u32, Box<dyn Error>> = s.parse::<u32>().map_err(|e| e.into());
    audit().await; // `parsed` (maybe an Err(Box<dyn Error>)) is live across the await
    parsed
}

fn main() {
    let fut = parse_amount("42");
    std::thread::spawn(move || {
        let _ = futures::executor::block_on(fut); // the result stays on that thread
    });
}
