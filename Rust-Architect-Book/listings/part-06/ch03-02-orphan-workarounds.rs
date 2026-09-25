// verify: debug ok
use std::fmt;
use std::ops::Deref;

// Workaround 1: a NEWTYPE makes the type local. Local type + foreign trait: allowed.
struct Hex(Vec<u8>);

impl fmt::Display for Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in &self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl Deref for Hex {
    type Target = [u8]; // keep slice methods available on the wrapper
    fn deref(&self) -> &[u8] {
        &self.0
    }
}

// Workaround 2: an EXTENSION TRAIT makes the trait local. Local trait + foreign type: allowed.
trait Checksum {
    fn checksum(&self) -> u32;
}

impl Checksum for [u8] {
    fn checksum(&self) -> u32 {
        self.iter().fold(0u32, |acc, &b| acc.rotate_left(5) ^ u32::from(b))
    }
}

fn main() {
    let frame = Hex(vec![0xCA, 0xFE, 0x01]);
    println!("frame = {frame}, len = {}", frame.len()); // len() via Deref to [u8]
    println!("checksum = {:#010x}", frame.checksum()); // extension trait on [u8], reached through Deref
    println!("checksum of literal = {:#010x}", b"MRDN".checksum());
}
