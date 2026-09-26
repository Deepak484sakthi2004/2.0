// verify: debug ok
// Listing 17.2-6: a lexer is a DFA. This is the shape of what lexer generators (lex/flex, re2c,
// Rust's `logos` crate) produce: a byte -> class table, a (state, class) -> state table, and a
// "longest match" loop that remembers the last accepting state. Both tables are built at compile time.

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind { Ident, Int, Op, Error }

// Character classes.
const LETTER: u8 = 0; const DIGIT: u8 = 1; const LT: u8 = 2; const GT: u8 = 3; const EQ: u8 = 4;
const BANG: u8 = 5; const MINUS: u8 = 6; const AMP: u8 = 7; const PIPE: u8 = 8; const SINGLE: u8 = 9;
const OTHER: u8 = 10;
const NCLASS: usize = 11;

// States. 255 = dead (no transition).
const START: u8 = 0; const S_IDENT: u8 = 1; const S_INT: u8 = 2; const S_LT: u8 = 3; const S_GT: u8 = 4;
const S_EQ: u8 = 5; const S_BANG: u8 = 6; const S_MINUS: u8 = 7; const S_AMP: u8 = 8; const S_PIPE: u8 = 9;
const S_TWO: u8 = 10; const S_SINGLE: u8 = 11;
const NSTATE: usize = 12;
const DEAD: u8 = 255;

const fn build_classes() -> [u8; 256] {
    let mut t = [OTHER; 256];
    let mut b = 0;
    while b < 256 {
        let c = b as u8;
        t[b] = if c.is_ascii_alphabetic() || c == b'_' { LETTER }
            else if c.is_ascii_digit() { DIGIT }
            else if c == b'<' { LT } else if c == b'>' { GT } else if c == b'=' { EQ }
            else if c == b'!' { BANG } else if c == b'-' { MINUS } else if c == b'&' { AMP }
            else if c == b'|' { PIPE }
            else if matches!(c, b'+' | b'*' | b'/' | b'%' | b'(' | b')' | b'{' | b'}' | b',' | b';' | b':') { SINGLE }
            else { OTHER };
        b += 1;
    }
    t
}

const fn build_table() -> [[u8; NCLASS]; NSTATE] {
    let mut t = [[DEAD; NCLASS]; NSTATE];
    let s = START as usize;
    t[s][LETTER as usize] = S_IDENT;
    t[s][DIGIT as usize] = S_INT;
    t[s][LT as usize] = S_LT;
    t[s][GT as usize] = S_GT;
    t[s][EQ as usize] = S_EQ;
    t[s][BANG as usize] = S_BANG;
    t[s][MINUS as usize] = S_MINUS;
    t[s][AMP as usize] = S_AMP;
    t[s][PIPE as usize] = S_PIPE;
    t[s][SINGLE as usize] = S_SINGLE;
    t[S_IDENT as usize][LETTER as usize] = S_IDENT;
    t[S_IDENT as usize][DIGIT as usize] = S_IDENT;
    t[S_INT as usize][DIGIT as usize] = S_INT;
    t[S_LT as usize][EQ as usize] = S_TWO; // <=
    t[S_GT as usize][EQ as usize] = S_TWO; // >=
    t[S_EQ as usize][EQ as usize] = S_TWO; // ==
    t[S_BANG as usize][EQ as usize] = S_TWO; // !=
    t[S_MINUS as usize][GT as usize] = S_TWO; // ->
    t[S_AMP as usize][AMP as usize] = S_TWO; // &&
    t[S_PIPE as usize][PIPE as usize] = S_TWO; // ||
    t
}

static CLASSES: [u8; 256] = build_classes();
static TABLE: [[u8; NCLASS]; NSTATE] = build_table();
/// What each state accepts, if the input stops there. A lone `&` or `|` accepts nothing.
static ACCEPT: [Option<Kind>; NSTATE] = [
    None, Some(Kind::Ident), Some(Kind::Int), Some(Kind::Op), Some(Kind::Op), Some(Kind::Op),
    Some(Kind::Op), Some(Kind::Op), None, None, Some(Kind::Op), Some(Kind::Op),
];

fn lex_dfa(src: &[u8]) -> Vec<(Kind, usize, usize)> {
    let (mut out, mut i) = (Vec::new(), 0);
    while i < src.len() {
        if src[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let (mut state, mut j, mut last) = (START, i, None);
        while j < src.len() {
            let next = TABLE[state as usize][CLASSES[src[j] as usize] as usize];
            if next == DEAD {
                break;
            }
            state = next;
            j += 1;
            if let Some(k) = ACCEPT[state as usize] {
                last = Some((k, j)); // longest match so far
            }
        }
        match last {
            Some((k, end)) => {
                out.push((k, i, end));
                i = end;
            }
            None => {
                out.push((Kind::Error, i, i + 1));
                i += 1;
            }
        }
    }
    out
}

/// The hand-written equivalent, for comparison (same rules as listing 17.2-2's scanner).
fn lex_by_hand(src: &[u8]) -> Vec<(Kind, usize, usize)> {
    let (mut out, mut i) = (Vec::new(), 0);
    while i < src.len() {
        let (c, start) = (src[i], i);
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let kind = if c.is_ascii_digit() {
            while i < src.len() && src[i].is_ascii_digit() {
                i += 1;
            }
            Kind::Int
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < src.len() && (src[i].is_ascii_alphanumeric() || src[i] == b'_') {
                i += 1;
            }
            Kind::Ident
        } else {
            let two = matches!(
                (c, src.get(i + 1)),
                (b'<' | b'>' | b'=' | b'!', Some(b'=')) | (b'-', Some(b'>')) | (b'&', Some(b'&')) | (b'|', Some(b'|'))
            );
            let ok = two || CLASSES[c as usize] != OTHER && c != b'&' && c != b'|';
            i += if two { 2 } else { 1 };
            if ok { Kind::Op } else { Kind::Error }
        };
        out.push((kind, start, i));
    }
    out
}

fn main() {
    println!("tables: {} bytes of classes + {}x{} transitions = {} bytes",
        CLASSES.len(), NSTATE, NCLASS, CLASSES.len() + NSTATE * NCLASS);
    let src = "fn f(a1: int) -> int { a1<=b->c && d||!e == 42 }  x & y";
    let toks = lex_dfa(src.as_bytes());
    for (k, lo, hi) in &toks {
        print!("{}:{:?}  ", &src[*lo..*hi], k);
    }
    println!();
    assert_eq!(toks, lex_by_hand(src.as_bytes()));

    // A larger differential check: both lexers must agree on every input.
    let mut seed = 0x2545F4914F6CDD1Du64;
    let alphabet = b"ab1_ <=>!-&|+*(){};:\n";
    for _ in 0..2_000 {
        let s: Vec<u8> = (0..40)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                alphabet[(seed % alphabet.len() as u64) as usize]
            })
            .collect();
        assert_eq!(lex_dfa(&s), lex_by_hand(&s), "disagree on {:?}", String::from_utf8_lossy(&s));
    }
    println!("DFA and hand-written lexers agree on 2,000 random inputs");
}
