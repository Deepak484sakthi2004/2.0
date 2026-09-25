// verify: debug error:E0277
fn total_cents(lines: &[&str]) -> Result<u64, std::num::ParseIntError> {
    let mut total = 0;
    lines.iter().for_each(|l| {
        total += l.parse::<u64>()?; // `?` returns from the CLOSURE, which returns (), not from total_cents
    });
    Ok(total)
}

fn main() {
    println!("{:?}", total_cents(&["1", "2"]));
}
