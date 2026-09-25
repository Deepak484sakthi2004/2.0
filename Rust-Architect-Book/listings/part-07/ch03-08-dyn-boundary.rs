// verify: debug ok
use std::io::{self, Write};

/// Hot path stays generic: called per record, benefits from inlining.
#[inline]
fn encode_field<W: std::fmt::Write>(w: &mut W, key: &str, value: u64) -> std::fmt::Result {
    write!(w, "{key}={value};")
}

/// Cold boundary takes `&mut dyn Write`: one copy of this function, whatever the sink is.
fn flush_report(sink: &mut dyn Write, lines: &[String]) -> io::Result<()> {
    for line in lines {
        sink.write_all(line.as_bytes())?;
        sink.write_all(b"\n")?;
    }
    sink.flush()
}

fn main() -> io::Result<()> {
    let mut lines = Vec::new();
    for (i, v) in [120u64, 7, 9_000].iter().enumerate() {
        let mut s = String::new();
        encode_field(&mut s, "shard", i as u64).unwrap();
        encode_field(&mut s, "p99_us", *v).unwrap();
        lines.push(s);
    }
    let mut file_like: Vec<u8> = Vec::new(); // one sink type
    flush_report(&mut file_like, &lines)?;
    flush_report(&mut io::stdout().lock(), &lines)?; // another sink type, same machine code
    println!("buffered {} bytes", file_like.len());
    Ok(())
}
