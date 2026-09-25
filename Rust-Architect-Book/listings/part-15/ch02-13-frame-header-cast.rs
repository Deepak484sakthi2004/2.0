// verify: release ok
// verify: debug crash misaligned
// verify: debug miri alignment
// "Zero-copy" parsing by casting bytes to a struct pointer: two bugs in one line.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct FrameHeader {
    magic: u16, // 0xCAFE, big-endian on the wire
    version: u8,
    flags: u8,
    body_len: u32, // big-endian on the wire
}

fn parse_cast(buf: &[u8]) -> FrameHeader {
    assert!(buf.len() >= size_of::<FrameHeader>());
    unsafe { *(buf.as_ptr() as *const FrameHeader) } // BUG 1: alignment. BUG 2: byte order.
}

/// The read buffer: 8-aligned, like a pooled buffer. Byte 0 is a channel tag; the header follows.
#[repr(C, align(8))]
struct ReadBuf([u8; 16]);

fn main() {
    let rb = ReadBuf([9, 0xCA, 0xFE, 1, 0, 0, 0, 0, 42, 0, 0, 0, 0, 0, 0, 0]);
    let frame = &rb.0[1..]; // header starts at offset 1: odd address
    let h = parse_cast(frame);
    println!("magic={:#06x} version={} body_len={}", h.magic, h.version, h.body_len);
}
