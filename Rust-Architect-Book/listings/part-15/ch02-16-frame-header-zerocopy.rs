// verify: debug ok
// verify: debug miri-ok
// Zero-copy done soundly: zerocopy checks size and alignment at the cast, and the big-endian field
// types (align 1) make byte order and alignment part of the TYPE instead of a comment.
use zerocopy::byteorder::{BigEndian, U16, U32};
use zerocopy::{FromBytes, Immutable, KnownLayout, Unaligned};

#[derive(FromBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
struct FrameHeader {
    magic: U16<BigEndian>,
    version: u8,
    flags: u8,
    body_len: U32<BigEndian>,
}

fn parse(buf: &[u8]) -> Option<&FrameHeader> {
    let (h, _body) = FrameHeader::ref_from_prefix(buf).ok()?; // borrows `buf`: no copy
    Some(h)
}

#[repr(C, align(8))]
struct ReadBuf([u8; 16]);

fn main() {
    let rb = ReadBuf([9, 0xCA, 0xFE, 1, 0, 0, 0, 0, 42, 0, 0, 0, 0, 0, 0, 0]);
    let h = parse(&rb.0[1..]).expect("short frame");
    println!(
        "magic={:#06x} version={} flags={} body_len={} (align_of FrameHeader = {})",
        h.magic.get(),
        h.version,
        h.flags,
        h.body_len.get(),
        align_of::<FrameHeader>()
    );
    println!("7-byte buffer: {}", parse(&rb.0[1..8]).is_none());
}
