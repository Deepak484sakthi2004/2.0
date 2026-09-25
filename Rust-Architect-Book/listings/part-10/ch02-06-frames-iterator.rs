// verify: debug ok
// A custom zero-copy iterator: length-prefixed frames in a byte buffer (the Chapter 2.4 frame format, simplified:
// 2-byte magic 0xCAFE, 1-byte type, 4-byte big-endian body length, then the body).
use std::iter::FusedIterator;

#[derive(Debug)]
struct Frame<'a> {
    kind: u8,
    body: &'a [u8],
}

struct Frames<'a> {
    buf: &'a [u8],
    failed: bool,
}

const HEADER: usize = 7;

impl<'a> Iterator for Frames<'a> {
    type Item = Result<Frame<'a>, String>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.buf.is_empty() {
            return None;
        }
        if self.buf.len() < HEADER || self.buf[..2] != [0xCA, 0xFE] {
            self.failed = true; // a framing error can't be resynchronized: stop for good after reporting it
            return Some(Err(format!("bad header, {} bytes left", self.buf.len())));
        }
        let len = u32::from_be_bytes(self.buf[3..7].try_into().unwrap()) as usize;
        let Some(body) = self.buf.get(HEADER..HEADER + len) else {
            self.failed = true;
            return Some(Err(format!("truncated body: want {len}, have {}", self.buf.len() - HEADER)));
        };
        let frame = Frame { kind: self.buf[2], body };
        self.buf = &self.buf[HEADER + len..];
        Some(Ok(frame))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.failed || self.buf.is_empty() {
            (0, Some(0))
        } else {
            // at least one item (a frame or an error); at most one per 7-byte header
            (1, Some(self.buf.len() / HEADER))
        }
    }
}

// We return None forever after the first None, so we can promise it (FusedIterator is a marker trait):
impl FusedIterator for Frames<'_> {}

fn frames(buf: &[u8]) -> Frames<'_> {
    Frames { buf, failed: false }
}

fn main() {
    let mut wire = Vec::new();
    for (kind, body) in [(1u8, &b"MRDN,12550"[..]), (2, b"HB"), (1, b"ACME,990")] {
        wire.extend_from_slice(&[0xCA, 0xFE, kind]);
        wire.extend_from_slice(&(body.len() as u32).to_be_bytes());
        wire.extend_from_slice(body);
    }
    wire.extend_from_slice(&[0xCA, 0xFE, 1, 0, 0, 0, 50, b'x']); // a truncated trailing frame

    println!("size_hint before: {:?}", frames(&wire).size_hint());
    for f in frames(&wire) {
        match f {
            Ok(fr) => println!("kind {} body {:?}", fr.kind, std::str::from_utf8(fr.body).unwrap()),
            Err(e) => println!("error: {e}"),
        }
    }
    // Every adapter works on our iterator for free:
    let quotes: Vec<&[u8]> = frames(&wire).filter_map(Result::ok).filter(|f| f.kind == 1).map(|f| f.body).collect();
    println!("quote frames: {}", quotes.len());
    println!("size_of::<Frames>() = {} B", std::mem::size_of::<Frames>());
}
