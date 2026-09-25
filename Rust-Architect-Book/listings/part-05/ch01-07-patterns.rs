// verify: debug ok
use std::convert::Infallible;
use std::mem::size_of;

#[derive(Debug)]
enum Frame<'a> {
    Ping,
    Data { stream: u32, payload: &'a [u8] },
    Close { code: u16 },
}

fn describe(f: &Frame) -> String {
    match f {
        Frame::Ping => "ping".to_string(),
        // binding + guard + @-binding on a range
        Frame::Data { stream: s @ 1..=15, payload } if payload.len() <= 4 => format!("small data on control stream {s}"),
        Frame::Data { stream, payload: [first, .., last] } => format!("data on {stream}: {first:#04x}..{last:#04x}"),
        Frame::Data { stream, payload: [] | [_] } => format!("tiny data on {stream}"),
        Frame::Close { code: 1000 } => "normal close".to_string(),
        Frame::Close { code } => format!("close with {code}"),
    }
}

// A config value that can never fail to parse: the error type has no values.
fn parse_mode(raw: &str) -> Result<String, Infallible> {
    Ok(raw.to_ascii_lowercase())
}

fn port_of(addr: &str) -> Option<u16> {
    // let-else: the happy path stays unindented
    let Some((_, port)) = addr.rsplit_once(':') else {
        return None;
    };
    port.parse().ok()
}

fn main() {
    let frames = [
        Frame::Ping,
        Frame::Data { stream: 3, payload: b"ok" },
        Frame::Data { stream: 40, payload: b"hello" },
        Frame::Data { stream: 40, payload: b"x" },
        Frame::Close { code: 1000 },
        Frame::Close { code: 4001 },
    ];
    for f in &frames {
        println!("{}", describe(f));
    }

    // Since 1.82 an Err(Infallible) arm may be omitted: the pattern is irrefutable.
    let Ok(mode) = parse_mode("STRICT");
    println!("mode = {mode}");

    // let chains (edition 2024, stable since 1.88)
    let addr = "10.0.0.7:8443";
    if let Some(p) = port_of(addr) && p >= 1024 {
        println!("{addr}: unprivileged port {p}");
    }

    println!(
        "size_of: Result<u64, Infallible> = {}, Option<u64> = {}, Frame = {}",
        size_of::<Result<u64, Infallible>>(),
        size_of::<Option<u64>>(),
        size_of::<Frame>()
    );
}
