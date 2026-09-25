// verify: debug ok
use std::io::{BufWriter, Write};

fn main() {
    let mut out = BufWriter::new(std::io::stdout());
    for i in 0..3 {
        writeln!(out, "exported row {i}").unwrap();
    }
    // BUG: process::exit ends the process WITHOUT running destructors,
    // so the BufWriter is never flushed and the rows are lost.
    std::process::exit(0);
}
