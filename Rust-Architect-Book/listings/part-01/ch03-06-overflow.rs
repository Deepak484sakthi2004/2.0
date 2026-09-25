// verify: release ok
// verify: debug panic attempt to add with overflow
use std::hint::black_box;

fn main() {
    let x: u8 = black_box(255);
    println!("wrapping_add:    {}", x.wrapping_add(1));
    println!("checked_add:     {:?}", x.checked_add(1));
    println!("saturating_add:  {}", x.saturating_add(1));
    println!("overflowing_add: {:?}", x.overflowing_add(1));
    println!("plain `+`:       {}", x + 1);
}
