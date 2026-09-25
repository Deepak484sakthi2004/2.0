// verify: debug crash aborting now
use std::io::{BufWriter, Write};

struct Span(&'static str);

impl Drop for Span {
    fn drop(&mut self) {
        println!("span {} closed", self.0); // never printed: abort runs no destructors
    }
}

// What `panic = "abort"` does to every panic, shown with process::abort (a profile can't be set on the Playground).
fn main() {
    let _span = Span("export");
    let mut out = BufWriter::new(std::io::stdout());
    writeln!(out, "row 1 (buffered, never flushed)").unwrap();
    println!("about to abort");
    eprintln!("aborting now");
    std::process::abort();
}
