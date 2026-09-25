// verify: debug ok
// verify: debug miri-ok
// The same parse with no unsafe: explicit byte order, no alignment requirement.
#[derive(Debug, Clone, Copy)]
struct FrameHeader {
    magic: u16,
    version: u8,
    flags: u8,
    body_len: u32,
}

fn parse(buf: &[u8]) -> Option<FrameHeader> {
    let b: &[u8; 8] = buf.get(..8)?.try_into().ok()?;
    Some(FrameHeader {
        magic: u16::from_be_bytes([b[0], b[1]]),
        version: b[2],
        flags: b[3],
        body_len: u32::from_be_bytes([b[4], b[5], b[6], b[7]]),
    })
}

#[repr(C, align(8))]
struct ReadBuf([u8; 16]);

fn main() {
    let rb = ReadBuf([9, 0xCA, 0xFE, 1, 0, 0, 0, 0, 42, 0, 0, 0, 0, 0, 0, 0]);
    let h = parse(&rb.0[1..]).expect("short frame");
    println!("magic={:#06x} version={} flags={} body_len={}", h.magic, h.version, h.flags, h.body_len);
}
