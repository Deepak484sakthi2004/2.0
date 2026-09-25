// verify: debug ok
use std::io::{BufWriter, Write};

fn main() {
    let mut out = BufWriter::new(std::io::stdout());
    for i in 0..3 {
        writeln!(out, "exported row {i}").unwrap();
    }
    // Flush explicitly: it is the only way to SEE a write error, and it survives process::exit.
    if let Err(e) = out.flush() {
        eprintln!("export failed: {e}");
        std::process::exit(1);
    }
    std::process::exit(0);
}
