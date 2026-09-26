// verify: debug error:E0373
//! tokio::spawn requires a 'static future: the task may outlive the function that spawned it.
#[tokio::main]
async fn main() {
    let merchant = String::from("m-1042");
    let task = tokio::spawn(async {
        println!("notifying {merchant}"); // borrows `merchant`, a local of main
    });
    task.await.unwrap();
}
