// verify: release ok
// Listing 17.2-2: what a token costs. The same token stream produced three ways:
//   A. bytes + spans (zero-copy; identifiers are &src[span])
//   B. bytes + an owned String per identifier/number token
//   C. a `Peekable<Chars>` lexer that builds every lexeme char by char (a common first attempt)
// Allocations are exact (counting allocator); timings are best-of-5, one Playground run: noisy.

mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    static ALLOCS: AtomicUsize = AtomicUsize::new(0);

    pub struct Counting;

    // SAFETY: both methods forward their exact arguments to `System`, which upholds the
    // GlobalAlloc contract; the counter is a plain atomic, so counting never allocates.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    #[global_allocator]
    static GLOBAL: Counting = Counting;

    pub fn allocs() -> usize {
        ALLOCS.load(Relaxed)
    }
}

use std::hint::black_box;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind { Ident, Int, Op }

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// A: zero-copy. Calls `emit(kind, lo, hi)` for each token; nothing is allocated.
fn lex_spans(src: &str, mut emit: impl FnMut(Kind, usize, usize)) {
    let b = src.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let start = i;
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let kind = if c.is_ascii_digit() {
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            Kind::Int
        } else if is_ident(c) {
            while i < b.len() && is_ident(b[i]) {
                i += 1;
            }
            Kind::Ident
        } else {
            i += if matches!((c, b.get(i + 1)), (b'-', Some(b'>')) | (b'=' | b'<' | b'>' | b'!', Some(b'='))) { 2 } else { 1 };
            Kind::Op
        };
        emit(kind, start, i);
    }
}

/// B: the same scanner, but each identifier/number token owns a copy of its text.
struct OwnedToken {
    kind: Kind,
    text: Option<String>,
}

fn lex_owned(src: &str) -> Vec<OwnedToken> {
    let mut out = Vec::new();
    lex_spans(src, |kind, lo, hi| {
        let text = (kind != Kind::Op).then(|| src[lo..hi].to_string());
        out.push(OwnedToken { kind, text });
    });
    out
}

/// C: chars().peekable(), pushing char by char into a fresh String for every lexeme.
fn lex_chars(src: &str) -> Vec<(Kind, String)> {
    let mut out = Vec::new();
    let mut it = src.chars().peekable();
    while let Some(&c) = it.peek() {
        if c.is_whitespace() {
            it.next();
            continue;
        }
        let mut s = String::new();
        let kind = if c.is_ascii_digit() {
            while let Some(&d) = it.peek().filter(|d| d.is_ascii_digit()) {
                s.push(d);
                it.next();
            }
            Kind::Int
        } else if c.is_alphanumeric() || c == '_' {
            while let Some(&d) = it.peek().filter(|d| d.is_alphanumeric() || **d == '_') {
                s.push(d);
                it.next();
            }
            Kind::Ident
        } else {
            s.push(c);
            it.next();
            if let Some(&n) = it.peek() {
                if (c == '-' && n == '>') || (matches!(c, '=' | '<' | '>' | '!') && n == '=') {
                    s.push(n);
                    it.next();
                }
            }
            Kind::Op
        };
        out.push((kind, s));
    }
    out
}

fn best_of<R>(runs: usize, mut f: impl FnMut() -> R) -> (f64, R) {
    let mut best = f64::MAX;
    let mut last = None;
    for _ in 0..runs {
        let t = Instant::now();
        let r = black_box(f());
        best = best.min(t.elapsed().as_secs_f64());
        last = Some(r);
    }
    (best, last.unwrap())
}

fn main() {
    // ~2 MB of synthetic Ore: the same function with distinct names.
    let mut src = String::new();
    let mut n = 0;
    while src.len() < 2_000_000 {
        src.push_str(&format!(
            "fn sum_to_{n}(limit_{n}: int) -> int {{\n    let mut total = 0;\n    while limit_{n} > 0 {{ total = total + limit_{n}; limit_{n} = limit_{n} - 1; }}\n    total\n}}\n"
        ));
        n += 1;
    }
    let mb = src.len() as f64 / 1e6;

    let mut count = 0usize;
    let a0 = counting::allocs();
    lex_spans(&src, |_, _, _| count += 1);
    let spans_allocs = counting::allocs() - a0;

    let a0 = counting::allocs();
    let mut collected = Vec::new();
    lex_spans(&src, |k, lo, hi| collected.push((k, lo as u32, hi as u32)));
    let collect_allocs = counting::allocs() - a0;

    let a0 = counting::allocs();
    let owned = lex_owned(&src);
    let owned_allocs = counting::allocs() - a0;

    let a0 = counting::allocs();
    let chars = lex_chars(&src);
    let chars_allocs = counting::allocs() - a0;

    let idents = owned.iter().filter(|t| t.kind == Kind::Ident).count();
    let with_text = owned.iter().filter(|t| t.text.is_some()).count();
    assert_eq!(collected.len(), count);
    assert_eq!(owned.len(), count);
    assert_eq!(chars.len(), count);
    println!("input {mb:.2} MB, {count} tokens ({idents} identifiers, {with_text} tokens with text)");
    println!("allocations:");
    println!("  A  spans, streamed             {spans_allocs:>8}");
    println!("  A' spans, collected into a Vec {collect_allocs:>8}   (Vec growth only)");
    println!("  B  owned String per lexeme     {owned_allocs:>8}");
    println!("  C  chars().peekable()          {chars_allocs:>8}");

    let (t_a, _) = best_of(5, || {
        let mut c = 0usize;
        lex_spans(&src, |k, lo, _| c += k as usize + lo);
        c
    });
    let (t_b, _) = best_of(5, || lex_owned(&src).len());
    let (t_c, _) = best_of(5, || lex_chars(&src).len());
    println!("throughput (release, best of 5, one run):");
    for (name, t) in [("A  spans", t_a), ("B  owned", t_b), ("C  chars", t_c)] {
        println!("  {name:<9} {:>7.1} MB/s   {:>6.2} ns/token", mb / t, t * 1e9 / count as f64);
    }
}
