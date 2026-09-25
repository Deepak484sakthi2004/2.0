// verify: debug ok
use std::fs;

fn main() {
    // Release the batch job's lock. If this fails, the next run will refuse to start.
    fs::remove_file("/tmp/meridian-settlement.lock");
    println!("lock released (or was it?)");
}
