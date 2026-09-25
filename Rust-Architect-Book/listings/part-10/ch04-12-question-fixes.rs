// verify: debug ok
// Answer-key check (Chapter 10.4 debugging exercise): three ways to use `?` with iteration.
use std::num::ParseIntError;

fn with_try_for_each(lines: &[&str]) -> Result<u64, ParseIntError> {
    let mut total = 0;
    lines.iter().try_for_each(|l| {
        total += l.parse::<u64>()?;
        Ok::<(), ParseIntError>(())
    })?;
    Ok(total)
}

fn with_sum(lines: &[&str]) -> Result<u64, ParseIntError> {
    lines.iter().map(|l| l.parse::<u64>()).sum()
}

fn with_loop(lines: &[&str]) -> Result<u64, ParseIntError> {
    let mut total = 0;
    for l in lines {
        total += l.parse::<u64>()?;
    }
    Ok(total)
}

fn main() {
    let good = ["1", "2", "39"];
    let bad = ["1", "x", "39"];
    println!("{:?} {:?} {:?}", with_try_for_each(&good), with_sum(&good), with_loop(&good));
    println!("{:?}", with_sum(&bad));
}
