// verify: debug crash overflowed its stack
use std::hint::black_box;
use std::thread;

fn checksum(buf: &[u8]) -> u64 {
    buf.iter().map(|&b| b as u64).sum()
}

fn main() {
    // Spawned threads get a 2 MiB stack by default; this array alone is 4 MiB.
    let handle = thread::spawn(|| {
        let buf = [1u8; 4 * 1024 * 1024];
        checksum(black_box(&buf))
    });
    println!("checksum = {}", handle.join().unwrap());
}
