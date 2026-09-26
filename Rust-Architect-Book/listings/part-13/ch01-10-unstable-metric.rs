// verify: debug error:E0599
//! Deeper scheduler metrics exist only when tokio is compiled with --cfg tokio_unstable.
#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let m = tokio::runtime::Handle::current().metrics();
    println!("{}", m.worker_steal_count(0));
}
