// verify: debug ok
use std::error::Error;
use std::mem::size_of;
use std::num::{NonZeroU32, ParseIntError};

macro_rules! show {
    ($t:ty) => {
        println!("{:<44} {:>3} bytes", stringify!($t), size_of::<$t>());
    };
}

fn main() {
    show!(Result<(), ()>);
    show!(Result<u64, ()>);
    show!(Result<&u64, ()>);
    show!(Result<NonZeroU32, ()>);
    show!(ParseIntError);
    show!(Result<u32, ParseIntError>);
    show!(std::io::Error);
    show!(Result<(), std::io::Error>);
    show!(Result<u64, std::io::Error>);
    show!(Box<dyn Error + Send + Sync>);
    show!(Result<(), Box<dyn Error + Send + Sync>>);
    show!(anyhow::Error);
    show!(Result<(), anyhow::Error>);
}
