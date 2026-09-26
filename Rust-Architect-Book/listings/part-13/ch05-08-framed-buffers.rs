// verify: release ok
//! What a framed connection costs before it has read a byte: tokio-util's FramedRead and FramedWrite each start
//! with an 8 KiB buffer (framed_impl.rs, tokio-util 0.7.19) [LIB]. Counting allocator (Part III's instrument).
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use tokio_util::codec::{BytesCodec, FramedRead, FramedWrite, LinesCodec};

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every method forwards to System with the same arguments and only adds bookkeeping.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size(), Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Relaxed);
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static A: Counting = Counting;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    const N: usize = 1_000;
    // The transports are created first, so only the framing layers are counted below.
    let mut streams: Vec<_> = (0..N).map(|_| (tokio::io::duplex(64), tokio::io::duplex(64))).collect();
    let mut framed = Vec::with_capacity(N);
    let before = LIVE.load(Relaxed);
    for ((a, _), (b, _)) in streams.drain(..) {
        framed.push((FramedRead::new(a, LinesCodec::new()), FramedWrite::new(b, BytesCodec::new())));
    }
    let per = (LIVE.load(Relaxed) - before) / N;
    println!("one idle FramedRead + FramedWrite pair: {per} bytes of buffers (plus the transport)");
    println!("x 100,000 connections: {:.2} GB", per as f64 * 100_000.0 / 1e9);
}
