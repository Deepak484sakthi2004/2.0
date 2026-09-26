// verify: debug ok
//! #[tokio::main] is a macro: it builds a multi-thread runtime with every driver enabled and block_on()s the body.
//! See its expansion with: tools/emit.ps1 listings/part-13/ch01-07-tokio-main.rs -Target expand -CrateType bin
#[tokio::main]
async fn main() {
    println!("hello from a task");
}
