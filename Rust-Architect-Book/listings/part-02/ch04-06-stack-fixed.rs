// verify: debug ok
use std::thread;

fn checksum(buf: &[u8]) -> u64 {
    buf.iter().map(|&b| b as u64).sum()
}

fn main() {
    let handle = thread::spawn(|| {
        let buf = vec![1u8; 4 * 1024 * 1024]; // 4 MiB on the heap; a 24-byte handle on the stack
        checksum(&buf)
    });
    println!("checksum = {}", handle.join().unwrap());
}
