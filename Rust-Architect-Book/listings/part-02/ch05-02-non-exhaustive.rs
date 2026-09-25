// verify: debug error:E0004
fn bucket(n: u8) -> &'static str {
    match n {
        0..=9 => "one digit",
        10..=99 => "two digits",
        100..=254 => "three digits",
    }
}

fn main() {
    println!("{}", bucket(7));
}
