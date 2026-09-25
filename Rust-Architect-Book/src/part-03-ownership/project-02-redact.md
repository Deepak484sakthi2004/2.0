# Project Level 2 — `redact`: A Streaming Secret-Masking Filter

> **Where this sits:** Part III · the second rung of the project ladder (a streaming file processor)
> **Uses:** ownership and moves (3.1–3.2), borrowing (3.3), `&str`, UTF-8, and `Cow` (3.4), RAII and flushing (3.5),
> plus Part II's CLI discipline (exit codes, buffered output, broken pipes).
> **Full source:** `listings/part-03/project-02-redact.rs`, verified with rustc 1.98.1: runs in debug and release, and
> **all 8 tests pass, including two that *measure* allocations**.

Logs leak secrets: card numbers pasted into support tickets, bearer tokens in debug lines, passwords in query strings,
customer emails everywhere. `redact` sits in a pipeline (`tail -F app.log | redact | ship-logs`) and masks them before
they leave the machine. It has to be correct, fast, and **cheap on the common case**, because 99% of log lines contain
nothing to mask.

The ownership goal is precise, and the tests check it: **a line with nothing to redact is written out without a single
allocation, and a line that changes costs exactly one.**

---

## 1. Requirements

```text
usage: redact [-q] [FILE ...]
  stdin when no FILE is given; "-" also means stdin; a summary goes to stderr unless -q
```

| Masks | Example in | Example out |
|---|---|---|
| E-mail addresses | `login ada@example.com` | `login <email>` |
| Card numbers: 13–19 contiguous digits **that pass the Luhn check** | `card=4242424242424242` | `card=<card:4242>` |
| Bearer tokens | `Authorization: Bearer eyJhbGciOi.x.y` | `Authorization: Bearer <redacted>` |
| Password parameters | `?user=ada&password=hunter2&next=/` | `?user=ada&password=<redacted>&next=/` |

**Non-functional:**

| Requirement | Why |
|---|---|
| Streaming, bounded memory | It runs in front of `tail -F` forever |
| **Zero allocations for clean lines** | Most lines are clean; per-line allocation would dominate CPU at high log rates |
| Never panic on input | Invalid UTF-8, binary garbage, and non-ASCII text must all pass through safely |
| Few false positives | Order IDs of 13–19 digits mustn't be masked: that's what the Luhn check is for |
| Correct exit codes, broken-pipe tolerance, explicit flush | Part II's CLI rules, and Chapter 3.5's flush lesson |

## 2. Design

```text
 stdin / files ──► process():  ONE reusable Vec<u8> line buffer (the only allocation for clean input)
                        │
                        │  bytes ──from_utf8_lossy──► Cow<str>     Borrowed if valid UTF-8 (no allocation)
                        │                                           Owned with U+FFFD if not (counted)
                        ▼
                  redact(&str) ──► Cow<'_, str>                    Borrowed: nothing matched (no allocation)
                        │                                           Owned: exactly one String, built lazily
                        ▼
                  output.write_all(bytes)  ──►  BufWriter<StdoutLock>  ──► flush() explicitly, then exit
```

**Ownership decisions, and why:**

| Value | Owner | Borrowed by | Why |
|---|---|---|---|
| Line buffer `Vec<u8>` | `process` | `&[u8]` body, then `&str` | Reused for every line: one allocation per run |
| Decoded text | The buffer (a `Cow::Borrowed` view) or a new `String` (only for invalid UTF-8) | `redact` | Lossy decoding is itself a `Cow` |
| Redacted line | Nobody (`Cow::Borrowed`) or one `String` | The writer, briefly | Allocation happens only when something is masked |
| Stats | `main` | `&mut Stats` through the call chain | One owner, mutable access passed down |

## 3. Implementation walkthrough

### Scanning bytes while staying UTF-8 safe

The scanner walks the line **byte by byte** and looks for a match *starting at* each byte:

```rust,ignore
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
    // ... secrets (known prefixes) and e-mail (local@domain.tld): see the full listing
    None
}
```

This is Chapter 3.4's **self-synchronization** property doing real work. Scanning `&[u8]` is fast and simple, but slicing
the `&str` at an arbitrary byte offset could panic (the "Kristina Øberg" bug). The fix isn't to scan by `char`, which is
slower and not needed. It's the invariant stated in the comment: **matches only start and end at ASCII bytes, and ASCII
bytes are always char boundaries.** So every slice `redact` takes is valid for any input, including "Zoë paid with
4242424242424242 ✓", and the `utf8_around_hits_is_preserved` test pins it down.

The scanner returns a small enum (`Hit::Card { end, last4: [u8; 4] }` and so on), plain `Copy`-able data with no
allocation. It says *what* to write without writing it.

### Lazy allocation with `Cow`

```rust,ignore
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
            scan::Hit::Email { end } => { stats.emails += 1; o.push_str("<email>"); end }
            scan::Hit::Card { end, last4 } => { /* "<card:NNNN>" */ end }
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
```

Three ownership points:

1. **The signature `fn redact<'a>(line: &'a str, ...) -> Cow<'a, str>`** says the result may borrow from the input line,
   so the caller must keep the line alive while using the result, which `process` does. (Part IV explains how the
   compiler checks this.)
2. **`out: Option<String>` plus `get_or_insert_with`** is the lazy-allocation pattern. The `String` is created at the
   first hit, never for clean lines. It's sized `line.len() + 16` up front, so in practice there's exactly **one**
   allocation, with no reallocation as replacements are pushed.
3. **`copied`** tracks how much of the input has already been copied into `out`. Unchanged spans are copied in bulk
   (`push_str(&line[copied..i])`), not byte by byte.

### Streaming with one buffer

```rust,ignore
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
```

Two `Cow`s are chained here. `from_utf8_lossy` borrows the buffer when the bytes are valid UTF-8, and `redact` borrows
*that* when nothing matches. In the common case, **both are views into `buf`**, and the line reaches the output with no
allocation at all. The borrows end before the next `buf.clear()`, so the reuse is checked by the compiler (Chapter 3.3):
try holding `redacted` across iterations and the build fails.

`main` wraps stdout in `BufWriter::new(io::stdout().lock())`, runs everything through a `run` function, **flushes
explicitly** (Chapter 3.5: `Drop` would swallow the error, and `process::exit` would skip it), treats `BrokenPipe` as
success, and returns an `ExitCode`: 0 on success, 1 on I/O errors, 2 on usage errors.

## 4. Testing, including allocation tests

The listing's test module installs a **test-only counting allocator** (`#[cfg(test)] #[global_allocator]`). It counts
allocations **per thread**, so tests running in parallel don't disturb each other's counts. The counter is a
const-initialized thread-local with no destructor, so counting never allocates itself, and the `unsafe impl GlobalAlloc`
carries its safety argument in a comment, as Part XV will require of every `unsafe` block. All 8 tests pass:

```text
running 8 tests
test tests::clean_lines_are_borrowed_and_never_allocate ... ok
test tests::email_edge_cases ... ok
test tests::luhn_spares_lookalike_order_ids ... ok
test tests::dirty_lines_allocate_exactly_once ... ok
test tests::masks_secrets_but_keeps_prefixes ... ok
test tests::stream_golden ... ok
test tests::utf8_around_hits_is_preserved ... ok
test tests::streaming_clean_input_allocates_once_in_total ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
```

The ownership claims, as assertions:

```rust,ignore
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
```

**Ten thousand lines, one allocation**: the line buffer. And a line that needs masking costs exactly one
(`dirty_lines_allocate_exactly_once`). Note that `12345` in the first test is a five-digit run, not a card, and
`user=ada` isn't an email. The golden stream test covers the mix, including invalid UTF-8 (`\xff\xfe` becomes two
U+FFFD characters and is counted) and a final line with no trailing newline:

```text
input:   GET /health 200\nlogin ada@example.com password=s3cr3t\n\xff\xfe binary junk\ncard 4111111111111111 ok
output:  GET /health 200\nlogin <email> password=<redacted>\n\u{FFFD}\u{FFFD} binary junk\ncard <card:1111> ok
stats:   lines 4, changed 2, e-mail 1, card 1, secret 1, invalid UTF-8 1
```

Other tests pin down the false-positive rules. `1234567890123` (13 digits, fails Luhn) and a 20-digit run are left
alone. `a@b.c` (one-letter TLD) and `user@localhost` (no dot) aren't emails. A trailing period after an address is
punctuation, not part of the domain.

## 5. Production concerns

- **Detection is heuristic, and that must be stated.** Luhn catches most random digit runs but not all: about 1 in 10
  random 16-digit numbers passes. Emails with non-ASCII domains (IDNs) aren't detected by this ASCII scanner. A
  redaction tool is one layer of defense, not a guarantee. Structured logging that never writes secrets in the first
  place is the real fix.
- **Cost profile.** Per byte, the scanner does a few comparisons. On clean text, `hit_at` returns quickly at each
  position, so the work is roughly linear in input size, with no allocation. That's a hypothesis until profiled (Part
  XX). The obvious optimization, if a profile demands it, is to jump between candidate bytes (digits, `@`, `B`, `p`)
  with `memchr` instead of testing every position.
- **Lines are unbounded.** A single 2 GB line with no newline makes the line buffer grow to 2 GB. The production fix is
  a maximum line length, beyond which the tool either processes in chunks (and accepts missing a secret split across a
  chunk boundary) or rejects the line. That's a genuine trade-off between memory and completeness.
- **Stats go to stderr, data to stdout**, so `redact < in.log > out.log` produces a clean file and a summary on the
  terminal.

## 6. Architecture review

*You're the principal engineer reviewing `redact` before it goes into every service's log pipeline. Model answers are
in Appendix A (Part III).*

1. **Allocation profile.** The tests prove one allocation per dirty line and zero per clean line. What *else* allocates
   in production that the tests don't cover (stdin locking, `BufWriter`, invalid UTF-8)? Is any of it per-line?
2. **Worst-case input.** What input makes `redact` slowest per byte? Is there any input that makes it super-linear?
   (Look closely at the e-mail scan and the digit-run scan.)
3. **Correctness boundaries.** A card number split across two lines, or across two `write` calls from the application,
   isn't masked. Where in the pipeline should that be solved, if at all?
4. **Unbounded lines.** Design the maximum-line-length behavior. What do you do with a 10 MB line, and what does the
   stats line report?
5. **Ownership boundary.** Why does `redact` return `Cow<'a, str>` instead of `String`? What would change if the output
   had to be *queued* to another thread instead of written immediately?
6. **Concurrency.** Sketch a multi-threaded version for a 10 GB file. What does each thread own, how do you preserve line
   order, and where does the "no sharing, merge at the end" pattern apply to `Stats`?
7. **`unsafe`.** Where is `unsafe` used, and why is it only in test code? Could `redact` itself benefit from `unsafe`
   (for example `from_utf8_unchecked`)? What invariant would you need, and is it worth it?
8. **Priorities.** How would the design change if **recall** (never miss a secret) mattered more than precision? If
   **latency** mattered most (line-at-a-time flushing for `tail -F`)? If **developer velocity** mattered most (a regex
   crate)?

## 7. Extensions

- **Beginner:** add a `--mask-char` option that replaces card digits with `*` except the last four
  (`************4242`). Does the `Cow` design still give zero allocations for clean lines?
- **Intermediate:** add IPv4 address masking (`10.0.0.12` becomes `<ip>`). Make sure version strings like `1.2.3.4`
  (inside a word) and out-of-range octets aren't masked.
- **Advanced:** replace the per-byte loop with `memchr`-driven candidate skipping and measure the speedup on a 1 GB mixed
  log. Keep all 8 tests passing.
- **Systems:** make output flushing adaptive: flush after every line when stdin is a terminal or a pipe from `tail -F`,
  and batch otherwise. How do you detect the difference (`std::io::IsTerminal`)?
- **Architecture:** turn the scanner into a library crate used by three consumers: this CLI, a Tokio log-shipping
  service (Part XIII), and a `tracing` layer that redacts *before* formatting (Part XXII). What should the library's
  API be, in terms of ownership?
