// verify: debug ok
/// Wire header: magic (2 bytes) | version (1) | flags (1) | body length (4, big-endian) = 8 bytes.
#[derive(Debug)]
struct Header {
    version: u8,
    flags: u8,
    body_len: u32,
}

const MAGIC: [u8; 2] = [0xCA, 0xFE];

fn parse_header(buf: &[u8]) -> Result<(Header, &[u8]), String> {
    if buf.len() < 8 {
        return Err(format!("need 8 header bytes, got {}", buf.len()));
    }
    let (head, rest) = buf.split_at(8);
    if head[0..2] != MAGIC {
        return Err(format!("bad magic {:#04x} {:#04x}", head[0], head[1]));
    }
    let len_bytes: [u8; 4] = head[4..8].try_into().expect("exactly 4 bytes");
    let header = Header { version: head[2], flags: head[3], body_len: u32::from_be_bytes(len_bytes) };
    Ok((header, rest))
}

fn main() {
    let frame = [0xCA, 0xFE, 1, 0b0000_0010, 0, 0, 0, 5, b'h', b'e', b'l', b'l', b'o'];
    match parse_header(&frame) {
        Ok((h, body)) => println!(
            "version={} flags={:#04b} body_len={} body={:?}",
            h.version, h.flags, h.body_len, std::str::from_utf8(body)
        ),
        Err(e) => println!("error: {e}"),
    }
    println!("{:?}", parse_header(&frame[..5]).map(|(h, _)| h.body_len));
    println!("{:?}", parse_header(&[0xBA, 0xAD, 1, 0, 0, 0, 0, 0]).map(|(h, _)| h.body_len));
}
