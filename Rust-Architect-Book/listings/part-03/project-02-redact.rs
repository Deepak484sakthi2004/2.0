// verify: debug ok
// verify: debug test
// verify: release ok
//! redact: stream text (logs), masking secrets before they leave the machine.
//!
//! Masks e-mail addresses -> <email>; 13-19 digit card numbers that pass the Luhn check -> <card:NNNN>
//! (last four kept); `Bearer <token>` -> `Bearer <redacted>`; `password=<value>` -> `password=<redacted>`.
//! Lines with nothing to mask are written through WITHOUT any allocation.
//!
//! Usage: redact [-q] [FILE ...]     (stdin when no FILE is given; "-" also means stdin;
//!                                    a summary goes to stderr unless -q)

use std::borrow::Cow;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::process::ExitCode;

const USAGE: &str = "usage: redact [-q] [FILE ...]\n";

#[derive(Debug, Default, PartialEq)]
pub struct Stats {
    pub lines: u64,
    pub changed_lines: u64,
    pub emails: u64,
    pub cards: u64,
    pub secrets: u64,
    pub invalid_utf8_lines: u64,
}

mod scan {
    /// A match that starts at some byte offset: where it ends, and what replaces it.
    pub enum Hit {
        Email { end: usize },
        Card { end: usize, last4: [u8; 4] },
        Secret { value_start: usize, end: usize }, // keep the prefix, mask the value
    }

    const SECRET_PREFIXES: [&[u8]; 2] = [b"Bearer ", b"password="];

    fn is_local_char(c: u8) -> bool {
        c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'%' | b'+' | b'-')
    }

    fn is_domain_char(c: u8) -> bool {
        c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-')
    }

    pub fn luhn_valid(digits: &[u8]) -> bool {
        let mut sum = 0;
        for (i, &d) in digits.iter().rev().enumerate() {
            let mut v = u32::from(d - b'0');
            if i % 2 == 1 {
                v *= 2;
                if v > 9 {
                    v -= 9;
                }
            }
            sum += v;
        }
        sum % 10 == 0
    }

    /// Looks for a hit starting exactly at byte `i` of a UTF-8 line.
    /// Only ASCII bytes can start (or end) a hit, and in UTF-8 an ASCII byte is ALWAYS a
    /// character boundary, so every offset this returns is safe to slice at.
    pub fn hit_at(b: &[u8], i: usize) -> Option<Hit> {
        if !b[i].is_ascii() {
            return None;
        }
        let prev = if i == 0 { None } else { Some(b[i - 1]) };

        // Card number: a whole run of 13-19 digits that passes the Luhn check.
        if b[i].is_ascii_digit() && !prev.is_some_and(|p| p.is_ascii_alphanumeric()) {
            let end = i + b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
            let run = &b[i..end];
            let next_is_word = b.get(end).is_some_and(|c| c.is_ascii_alphanumeric());
            if (13..=19).contains(&run.len()) && !next_is_word && luhn_valid(run) {
                let last4 = run[run.len() - 4..].try_into().expect("a run has at least 13 digits");
                return Some(Hit::Card { end, last4 });
            }
        }

        // Secret: a known prefix, then a non-empty value up to whitespace or '&'.
        for prefix in SECRET_PREFIXES {
            if b[i..].starts_with(prefix) {
                let value_start = i + prefix.len();
                let len = b[value_start..].iter().take_while(|&&c| !c.is_ascii_whitespace() && c != b'&').count();
                if len > 0 {
                    return Some(Hit::Secret { value_start, end: value_start + len });
                }
            }
        }

        // E-mail: local@domain.tld, starting at the beginning of a "word".
        if is_local_char(b[i]) && !prev.is_some_and(is_local_char) {
            let at = i + b[i..].iter().take_while(|&&c| is_local_char(c)).count();
            if b.get(at) == Some(&b'@') {
                let mut end = at + 1 + b[at + 1..].iter().take_while(|&&c| is_domain_char(c)).count();
                while end > at + 1 && matches!(b[end - 1], b'.' | b'-') {
                    end -= 1; // "mail me at ops@example.com." -> the final '.' is punctuation
                }
                let domain = &b[at + 1..end];
                if let Some(dot) = domain.iter().rposition(|&c| c == b'.') {
                    let tld = &domain[dot + 1..];
                    if dot > 0 && tld.len() >= 2 && tld.iter().all(u8::is_ascii_alphabetic) {
                        return Some(Hit::Email { end });
                    }
                }
            }
        }
        None
    }
}

/// Returns the line itself (borrowed: zero allocations) when nothing needs masking,
/// or a new String (exactly one allocation) when something does.
pub fn redact<'a>(line: &'a str, stats: &mut Stats) -> Cow<'a, str> {
    let bytes = line.as_bytes();
    let mut out: Option<String> = None; // allocated lazily, on the first hit
    let mut copied = 0; // bytes of `line` before this offset are already in `out`
    let mut i = 0;
    while i < bytes.len() {
        let Some(hit) = scan::hit_at(bytes, i) else {
            i += 1;
            continue;
        };
        let o = out.get_or_insert_with(|| String::with_capacity(line.len() + 16));
        o.push_str(&line[copied..i]);
        i = match hit {
            scan::Hit::Email { end } => {
                stats.emails += 1;
                o.push_str("<email>");
                end
            }
            scan::Hit::Card { end, last4 } => {
                stats.cards += 1;
                o.push_str("<card:");
                o.push_str(std::str::from_utf8(&last4).expect("ASCII digits"));
                o.push('>');
                end
            }
            scan::Hit::Secret { value_start, end } => {
                stats.secrets += 1;
                o.push_str(&line[i..value_start]); // keep "Bearer " / "password="
                o.push_str("<redacted>");
                end
            }
        };
        copied = i;
    }
    match out {
        None => Cow::Borrowed(line),
        Some(mut o) => {
            o.push_str(&line[copied..]);
            stats.changed_lines += 1;
            Cow::Owned(o)
        }
    }
}

/// Streams `input` to `output` line by line, reusing a single line buffer.
pub fn process(mut input: impl BufRead, output: &mut impl Write, stats: &mut Stats) -> io::Result<()> {
    let mut buf = Vec::with_capacity(4096);
    loop {
        buf.clear();
        if input.read_until(b'\n', &mut buf)? == 0 {
            return Ok(());
        }
        stats.lines += 1;
        let (body, newline): (&[u8], &[u8]) = match buf.strip_suffix(b"\n") {
            Some(body) => (body, b"\n"),
            None => (&buf, b""),
        };
        // Invalid UTF-8 becomes U+FFFD. from_utf8_lossy is itself a Cow: borrowed when the bytes are valid.
        let text = String::from_utf8_lossy(body);
        if matches!(text, Cow::Owned(_)) {
            stats.invalid_utf8_lines += 1;
        }
        let redacted = redact(&text, stats);
        output.write_all(redacted.as_bytes())?;
        output.write_all(newline)?;
    }
}

fn run(files: &[String], out: &mut impl Write, stats: &mut Stats) -> io::Result<()> {
    if files.is_empty() {
        process(io::stdin().lock(), out, stats)?;
    }
    for path in files {
        if path == "-" {
            process(io::stdin().lock(), out, stats)?;
        } else {
            let file = File::open(path).map_err(|e| io::Error::new(e.kind(), format!("{path}: {e}")))?;
            process(BufReader::new(file), out, stats)?;
        }
    }
    out.flush()
}

fn main() -> ExitCode {
    let mut quiet = false;
    let mut files = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-q" | "--quiet" => quiet = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            flag if flag.starts_with('-') && flag != "-" => {
                eprint!("redact: unknown option {flag:?}\n{USAGE}");
                return ExitCode::from(2);
            }
            _ => files.push(arg),
        }
    }

    let mut stats = Stats::default();
    let mut out = BufWriter::new(io::stdout().lock());
    let code = match run(&files, &mut out, &mut stats) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("redact: {e}");
            ExitCode::from(1)
        }
    };
    if !quiet {
        eprintln!(
            "redact: {} line(s), {} changed ({} e-mail, {} card, {} secret), {} with invalid UTF-8",
            stats.lines, stats.changed_lines, stats.emails, stats.cards, stats.secrets, stats.invalid_utf8_lines
        );
    }
    code
}

// --- test-only instrumentation: count allocations made by the CURRENT thread ---
#[cfg(test)]
mod alloc_count {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        static COUNT: Cell<usize> = const { Cell::new(0) };
    }

    pub struct Counting;

    // SAFETY: both methods forward their exact arguments to `System`, which upholds the GlobalAlloc
    // contract. The counter is a const-initialized thread-local without a destructor, so touching it
    // never allocates or re-enters the allocator; `try_with` makes access during thread teardown a no-op.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let _ = COUNT.try_with(|c| c.set(c.get() + 1));
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    #[global_allocator]
    static GLOBAL: Counting = Counting;

    pub fn allocations() -> usize {
        COUNT.with(|c| c.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(line: &str) -> String {
        redact(line, &mut Stats::default()).into_owned()
    }

    #[test]
    fn clean_lines_are_borrowed_and_never_allocate() {
        let line = "2026-09-25T10:00:00Z GET /api/orders/12345 200 37ms user=ada";
        let mut stats = Stats::default();
        let before = alloc_count::allocations();
        let out = redact(line, &mut stats);
        let allocated = alloc_count::allocations() - before;
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(allocated, 0);
    }

    #[test]
    fn dirty_lines_allocate_exactly_once() {
        let line = "charge card=4242424242424242 for ada@example.com";
        let mut stats = Stats::default();
        let before = alloc_count::allocations();
        let out = redact(line, &mut stats);
        let allocated = alloc_count::allocations() - before;
        assert_eq!(out, "charge card=<card:4242> for <email>");
        assert_eq!(allocated, 1);
        assert_eq!((stats.cards, stats.emails, stats.changed_lines), (1, 1, 1));
    }

    #[test]
    fn luhn_spares_lookalike_order_ids() {
        assert!(scan::luhn_valid(b"4111111111111111"));
        assert!(!scan::luhn_valid(b"1234567890123"));
        assert_eq!(r("order 1234567890123 shipped"), "order 1234567890123 shipped");
        assert_eq!(r("ids 42424242424242424242"), "ids 42424242424242424242"); // 20 digits: too long
    }

    #[test]
    fn masks_secrets_but_keeps_prefixes() {
        assert_eq!(r("Authorization: Bearer eyJhbGciOi.x.y"), "Authorization: Bearer <redacted>");
        assert_eq!(
            r("POST /login?user=ada&password=hunter2&next=/"),
            "POST /login?user=ada&password=<redacted>&next=/"
        );
    }

    #[test]
    fn utf8_around_hits_is_preserved() {
        assert_eq!(
            r("Zoë paid with 4242424242424242 ✓ zoe@example.com"),
            "Zoë paid with <card:4242> ✓ <email>"
        );
    }

    #[test]
    fn email_edge_cases() {
        assert_eq!(r("a@b.c"), "a@b.c"); // one-letter TLD: not an address
        assert_eq!(r("mail ops@meridian.example."), "mail <email>."); // trailing '.' is punctuation
        assert_eq!(r("user@localhost"), "user@localhost"); // no dot in the domain
    }

    #[test]
    fn stream_golden() {
        let input: &[u8] =
            b"GET /health 200\nlogin ada@example.com password=s3cr3t\n\xff\xfe binary junk\ncard 4111111111111111 ok";
        let mut out = Vec::new();
        let mut stats = Stats::default();
        process(input, &mut out, &mut stats).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "GET /health 200\nlogin <email> password=<redacted>\n\u{FFFD}\u{FFFD} binary junk\ncard <card:1111> ok"
        );
        let expected = Stats { lines: 4, changed_lines: 2, emails: 1, cards: 1, secrets: 1, invalid_utf8_lines: 1 };
        assert_eq!(stats, expected);
    }

    #[test]
    fn streaming_clean_input_allocates_once_in_total() {
        let input = "2026-09-25T10:00:00Z GET /api/orders 200 37ms\n".repeat(10_000);
        let mut out = Vec::with_capacity(input.len());
        let mut stats = Stats::default();
        let before = alloc_count::allocations();
        process(input.as_bytes(), &mut out, &mut stats).unwrap();
        let allocated = alloc_count::allocations() - before;
        assert_eq!(stats.lines, 10_000);
        assert_eq!(allocated, 1, "only the reusable line buffer");
    }
}
