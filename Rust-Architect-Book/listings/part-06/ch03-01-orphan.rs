// verify: debug error:E0117
use std::fmt;

// Both the trait (Display) and the type (Vec<u8>) come from std: neither is local to this crate.
impl fmt::Display for Vec<u8> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

fn main() {
    println!("{}", vec![0xCA_u8, 0xFE]);
}
