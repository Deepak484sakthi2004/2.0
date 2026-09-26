// verify: release ok
// Chapter 17.2's Systems exercise, in miniature: classify every byte of a lexer's input as whitespace, identifier,
// digit, punctuation, or other, with (a) a chain of comparisons and (b) one load from a 256-entry class table
// (the technique of Chapters 9.2 and 17.2). Three 1 MiB inputs: source-like text, a perfectly periodic input
// ("a b c d ..."), and a random mix of the same byte classes. Best of 7; one Playground run, noisy.
use std::hint::black_box;
use std::time::Instant;

const WS: usize = 0;
const IDENT: usize = 1;
const DIGIT: usize = 2;
const PUNCT: usize = 3;
const OTHER: usize = 4;

#[inline(always)]
fn classify_chain(b: u8) -> usize {
    if b == b' ' || b == b'\n' || b == b'\t' || b == b'\r' {
        WS
    } else if b.is_ascii_alphabetic() || b == b'_' {
        IDENT
    } else if b.is_ascii_digit() {
        DIGIT
    } else if b"(){}[];,.+-*/=<>!&|:\"#'".contains(&b) {
        PUNCT
    } else {
        OTHER
    }
}

const CLASSES: [u8; 256] = {
    let mut t = [OTHER as u8; 256];
    let mut i = 0;
    while i < 256 {
        let b = i as u8;
        t[i] = if b == b' ' || b == b'\n' || b == b'\t' || b == b'\r' {
            WS as u8
        } else if b.is_ascii_alphabetic() || b == b'_' {
            IDENT as u8
        } else if b.is_ascii_digit() {
            DIGIT as u8
        } else {
            let p = b"(){}[];,.+-*/=<>!&|:\"#'";
            let mut c = OTHER as u8;
            let mut j = 0;
            while j < p.len() {
                if p[j] == b {
                    c = PUNCT as u8;
                }
                j += 1;
            }
            c
        };
        i += 1;
    }
    t
};

#[inline(never)]
fn count_chain(v: &[u8]) -> [u64; 5] {
    let mut n = [0u64; 5];
    for &b in v {
        n[classify_chain(b)] += 1;
    }
    n
}

#[inline(never)]
fn count_table(v: &[u8]) -> [u64; 5] {
    let mut n = [0u64; 5];
    for &b in v {
        n[CLASSES[b as usize] as usize] += 1;
    }
    n
}

fn ns_per_byte(v: &[u8], f: fn(&[u8]) -> [u64; 5]) -> f64 {
    (0..7)
        .map(|_| {
            let t = Instant::now();
            black_box(f(black_box(v)));
            t.elapsed().as_nanos() as f64 / v.len() as f64
        })
        .fold(f64::MAX, f64::min)
}

fn main() {
    let n = 1 << 20;
    let snippet = b"fn parse_header(buf: &[u8]) -> Result<Header, Error> {\n    let len = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);\n    if len > 65_536 { return Err(Error::TooLong(len)); }\n}\n";
    let source: Vec<u8> = snippet.iter().copied().cycle().take(n).collect();
    let periodic: Vec<u8> = (0..n).map(|i| if i % 2 == 0 { b'a' + (i / 2 % 26) as u8 } else { b' ' }).collect();
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    let random: Vec<u8> = (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            snippet[(x % snippet.len() as u64) as usize] // same byte frequencies as `source`, random order
        })
        .collect();
    for v in [&source, &periodic, &random] {
        assert_eq!(count_chain(v), count_table(v));
    }
    println!("ns per byte (best of 7):          chain   table");
    for (name, v) in [("source-like text", &source), ("periodic \"a b c ...\"", &periodic), ("random, same byte mix", &random)] {
        println!("  {name:<28} {:6.3}  {:6.3}", ns_per_byte(v, count_chain), ns_per_byte(v, count_table));
    }
}
