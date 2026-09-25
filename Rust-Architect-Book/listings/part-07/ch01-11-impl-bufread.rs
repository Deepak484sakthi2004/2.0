// verify: debug ok
use std::io::{self, BufRead, BufReader, Cursor, Write};

/// Projects L1 and L2 used this shape: `impl BufRead` in, `impl Write` out.
/// Each distinct (reader, writer) pair a program uses becomes its own copy of this function.
fn number_lines(input: impl BufRead, out: &mut impl Write) -> io::Result<usize> {
    let mut n = 0;
    for line in input.lines() {
        n += 1;
        writeln!(out, "{n:>4} | {}", line?)?;
    }
    Ok(n)
}

fn main() -> io::Result<()> {
    // Instantiation 1: an in-memory byte slice, as in a unit test.
    let mut buf: Vec<u8> = Vec::new();
    number_lines(&b"alpha\nbeta\n"[..], &mut buf)?;
    print!("{}", String::from_utf8_lossy(&buf));

    // Instantiation 2: a buffered reader over something that implements Read, writing to stdout.
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    let n = number_lines(BufReader::new(Cursor::new("gamma\ndelta\nepsilon\n")), &mut lock)?;
    writeln!(lock, "{n} lines")?;
    Ok(())
}
