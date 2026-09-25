// verify: debug ok
#[derive(Debug)]
struct Header {
    version: u8,
    flags: u8,
    body_len: u32,
}

// The Chapter 2.4 parser, rewritten with slice patterns: length check,
// magic check, and field extraction are one match.
fn parse_header(buf: &[u8]) -> Result<(Header, &[u8]), String> {
    match buf {
        [0xCA, 0xFE, version, flags, l0, l1, l2, l3, body @ ..] => {
            let body_len = u32::from_be_bytes([*l0, *l1, *l2, *l3]);
            Ok((Header { version: *version, flags: *flags, body_len }, body))
        }
        [a, b, _, _, _, _, _, _, ..] => Err(format!("bad magic {a:#04x} {b:#04x}")),
        short => Err(format!("need 8 header bytes, got {}", short.len())),
    }
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
