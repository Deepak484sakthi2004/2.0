// verify: debug ok
//! How long is the kernel's accept queue behind tokio::net::TcpListener::bind? Read it from the source:
//! tokio's bind calls mio's bind, which calls listen() with a fixed backlog. TcpSocket::listen(n) sets your own.
use std::fs;

fn main() {
    let reg = "/playground/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f";
    let tokio = fs::read_to_string(format!("{reg}/tokio-1.53.1/src/net/tcp/listener.rs")).unwrap();
    for (i, l) in tokio.lines().enumerate().filter(|(_, l)| l.contains("mio::net::TcpListener::bind")) {
        println!("tokio-1.53.1/src/net/tcp/listener.rs:{}: {}", i + 1, l.trim());
    }
    let mio = fs::read_to_string(format!("{reg}/mio-1.2.3/src/net/tcp/listener.rs")).unwrap();
    for (i, l) in mio.lines().enumerate().skip(84).take(10) {
        println!("mio-1.2.3/src/net/tcp/listener.rs:{}: {}", i + 1, l.trim());
    }
}
