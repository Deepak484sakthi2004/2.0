// verify: debug error:E0658
// std's API for reading into UNINITIALIZED buffers (`BorrowedBuf`, `Read::read_buf`) is still unstable on
// Rust 1.98: `Read::read` takes `&mut [u8]`, which must be initialized, so buffers are zeroed first.
use std::io::BorrowedBuf;

fn main() {}
