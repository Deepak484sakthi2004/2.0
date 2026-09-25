// verify: debug ok
//! Async closures (stable since 1.85): `async |x| ...` and the AsyncFn* traits. Unlike a closure
//! that returns an async block, the returned future may borrow from the closure's captures.
async fn retry<F>(attempts: u32, op: F) -> Result<String, String>
where
    F: AsyncFn(u32) -> Result<String, String>,
{
    let mut last = Err("no attempts".to_string());
    for attempt in 1..=attempts {
        last = op(attempt).await;
        if last.is_ok() {
            break;
        }
    }
    last
}

fn main() {
    let endpoint = String::from("processor-eu-1");
    let log = std::cell::RefCell::new(Vec::new());
    let result = futures::executor::block_on(retry(3, async |attempt| {
        // Borrows `endpoint` and `log` from the enclosing scope; no clone, no Arc.
        log.borrow_mut().push(format!("attempt {attempt} -> {endpoint}"));
        if attempt < 3 { Err(format!("timeout on {endpoint}")) } else { Ok(format!("ch_{attempt}")) }
    }));
    println!("{result:?}");
    for line in log.borrow().iter() {
        println!("  {line}");
    }
}
