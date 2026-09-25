// verify: debug ok
use std::fmt;

#[derive(Debug)]
pub struct Header {
    pub version: u8,
    pub flags: u8,
    pub body_len: u32,
}

pub const HEADER_LEN: usize = 8;
pub const MAX_BODY: u32 = 1 << 20; // 1 MiB

// ---------------------------------------------------------------------------------------------
// Before (Chapters 2.4/2.5): errors are Strings, so the caller can only read the prose.
mod before {
    pub fn parse_header(buf: &[u8]) -> Result<u32, String> {
        match buf {
            [0xCA, 0xFE, _, _, l0, l1, l2, l3, ..] => Ok(u32::from_be_bytes([*l0, *l1, *l2, *l3])),
            [a, b, _, _, _, _, _, _, ..] => Err(format!("bad magic {a:#04x} {b:#04x}")),
            short => Err(format!("need 8 header bytes, got {}", short.len())),
        }
    }

    /// Six months later someone rewords the message. Nothing fails to compile.
    pub fn parse_header_reworded(buf: &[u8]) -> Result<u32, String> {
        match buf {
            [0xCA, 0xFE, _, _, l0, l1, l2, l3, ..] => Ok(u32::from_be_bytes([*l0, *l1, *l2, *l3])),
            [a, b, _, _, _, _, _, _, ..] => Err(format!("bad magic {a:#04x} {b:#04x}")),
            short => Err(format!("short read: {} of 8 header bytes", short.len())),
        }
    }

    /// The caller's only way to decide "wait for more bytes" vs "drop the connection".
    pub fn should_wait(e: &str) -> bool {
        e.contains("need")
    }
}

// ---------------------------------------------------------------------------------------------
// After: the error is data. Each variant is a distinct decision for the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HeaderError {
    /// Not enough bytes yet. Recoverable: read more and try again.
    Incomplete { needed: usize, got: usize },
    /// Not our protocol. Fatal for this connection.
    BadMagic { found: [u8; 2] },
    UnsupportedVersion(u8),
    BodyTooLarge { declared: u32, max: u32 },
}

impl fmt::Display for HeaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Incomplete { needed, got } => write!(f, "incomplete header: need {needed} bytes, got {got}"),
            Self::BadMagic { found: [a, b] } => write!(f, "bad magic {a:#04x} {b:#04x}"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported protocol version {v}"),
            Self::BodyTooLarge { declared, max } => write!(f, "declared body of {declared} bytes exceeds {max}"),
        }
    }
}

impl std::error::Error for HeaderError {}

impl HeaderError {
    pub fn is_incomplete(&self) -> bool {
        matches!(self, Self::Incomplete { .. })
    }
}

pub fn parse_header(buf: &[u8]) -> Result<(Header, &[u8]), HeaderError> {
    let Some((head, body)) = buf.split_first_chunk::<HEADER_LEN>() else {
        return Err(HeaderError::Incomplete { needed: HEADER_LEN, got: buf.len() });
    };
    let [m0, m1, version, flags, l0, l1, l2, l3] = *head;
    if [m0, m1] != [0xCA, 0xFE] {
        return Err(HeaderError::BadMagic { found: [m0, m1] });
    }
    if version != 1 {
        return Err(HeaderError::UnsupportedVersion(version));
    }
    let body_len = u32::from_be_bytes([l0, l1, l2, l3]);
    if body_len > MAX_BODY {
        return Err(HeaderError::BodyTooLarge { declared: body_len, max: MAX_BODY });
    }
    Ok((Header { version, flags, body_len }, body))
}

fn main() {
    let partial = [0xCA, 0xFE, 1, 0];
    println!("before:          should_wait = {}", before::should_wait(&before::parse_header(&partial).unwrap_err()));
    println!("before, reworded: should_wait = {}", before::should_wait(&before::parse_header_reworded(&partial).unwrap_err()));

    let inputs: [&[u8]; 5] = [
        &[0xCA, 0xFE, 1, 0b10, 0, 0, 0, 5, b'h', b'e', b'l', b'l', b'o'],
        &partial,
        &[0x47, 0x45, 0x54, 0x20, 0x2F, 0x20, 0x48, 0x54], // "GET / HT": someone sent HTTP
        &[0xCA, 0xFE, 2, 0, 0, 0, 0, 0],
        &[0xCA, 0xFE, 1, 0, 0xFF, 0xFF, 0xFF, 0xFF],
    ];
    for input in inputs {
        match parse_header(input) {
            Ok((h, body)) => println!("frame   v{} body_len={} body={:?}", h.version, h.body_len, std::str::from_utf8(body)),
            Err(e) if e.is_incomplete() => println!("wait    {e}"),
            Err(e) => println!("close   {e}"),
        }
    }
}
