// verify: debug crash Error:
use std::num::ParseIntError;

// main may return Result: an Err is printed with Debug to stderr and the exit status is 1.
fn main() -> Result<(), ParseIntError> {
    let port: u16 = "8080".parse()?;
    println!("port = {port}");
    let workers: u16 = "eight".parse()?;
    println!("workers = {workers}");
    Ok(())
}
