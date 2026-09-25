// verify: debug ok
// verify: release ok
use std::env::consts;
use std::mem::size_of;

fn main() {
    // Baked in at COMPILE time, by cargo (env!) and by rustc (cfg!, consts):
    println!("package:          {} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    println!("debug_assertions: {}", cfg!(debug_assertions));
    println!("target os/arch:   {}/{}", consts::OS, consts::ARCH);
    println!("pointer width:    {} bits", size_of::<usize>() * 8);
    // Read at RUN time, from the process environment:
    println!("RUST_LOG now:     {:?}", std::env::var("RUST_LOG").ok());
}
