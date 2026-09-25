// verify: debug ok
use std::mem::size_of;

/// FNV-1a over a fixed-size block: N is part of the TYPE, so each block size gets its own code.
fn checksum<const N: usize>(block: &[u8; N]) -> u32 {
    block.iter().fold(0x811c_9dc5u32, |h, &b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193))
}

/// A fixed-capacity ring buffer: no heap allocation, capacity known at compile time.
struct Ring<T: Copy + Default, const N: usize> {
    items: [T; N],
    head: usize,
    len: usize,
}

impl<T: Copy + Default, const N: usize> Ring<T, N> {
    fn new() -> Self {
        Ring { items: [T::default(); N], head: 0, len: 0 }
    }

    /// Pushes a value, overwriting the oldest one when full.
    fn push(&mut self, value: T) {
        let tail = (self.head + self.len) % N;
        self.items[tail] = value;
        if self.len < N {
            self.len += 1;
        } else {
            self.head = (self.head + 1) % N;
        }
    }

    fn iter(&self) -> impl Iterator<Item = T> + '_ {
        (0..self.len).map(move |i| self.items[(self.head + i) % N])
    }
}

fn main() {
    let header = [0xCA, 0xFE, 1, 0];
    let page = [7u8; 4096];
    println!("checksum::<4>    = {:#010x}", checksum(&header));
    println!("checksum::<4096> = {:#010x}", checksum(&page));

    let mut last_latencies: Ring<u32, 4> = Ring::new();
    for ms in [12, 40, 7, 95, 3, 61] {
        last_latencies.push(ms);
    }
    let kept: Vec<u32> = last_latencies.iter().collect();
    println!("last 4 latencies: {kept:?}");
    println!("size_of::<Ring<u32, 4>>()  = {} bytes (no heap)", size_of::<Ring<u32, 4>>());
    println!("size_of::<Ring<u64, 64>>() = {} bytes (no heap)", size_of::<Ring<u64, 64>>());
}
