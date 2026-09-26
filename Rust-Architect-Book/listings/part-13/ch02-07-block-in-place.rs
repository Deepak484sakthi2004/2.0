// verify: debug panic can call blocking only when running on the multi-threaded runtime
//! block_in_place turns the current worker into a blocking thread (and hands its queue to a new worker).
//! That needs other workers, so it panics on the current-thread runtime.
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let digest = tokio::task::block_in_place(|| {
        std::thread::sleep(std::time::Duration::from_millis(10)); // pretend: hash a statement file
        0xdeadbeef_u32
    });
    println!("{digest:x}");
}
