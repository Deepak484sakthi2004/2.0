// verify: debug ok
use std::time::Duration;

async fn handle(id: u32) -> u32 {
    tokio::time::sleep(Duration::from_millis(5)).await;
    if id == 2 {
        let empty: Vec<u32> = Vec::new();
        return empty[0]; // a bug in one request
    }
    id * 10
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    // Silence the default hook's stderr text so stdout shows the flow; a service would log here instead.
    std::panic::set_hook(Box::new(|_| {}));

    let tasks: Vec<_> = (1..=4).map(|id| (id, tokio::spawn(handle(id)))).collect();
    for (id, task) in tasks {
        match task.await {
            Ok(v) => println!("request {id}: ok {v}"),
            Err(e) if e.is_panic() => {
                let payload = e.into_panic();
                let msg = payload.downcast_ref::<String>().map(String::as_str).unwrap_or("?");
                println!("request {id}: task panicked ({msg}) → 500; runtime still up");
            }
            Err(e) => println!("request {id}: cancelled: {e}"),
        }
    }
    println!("after the panic: {:?}", tokio::spawn(handle(4)).await);
}
