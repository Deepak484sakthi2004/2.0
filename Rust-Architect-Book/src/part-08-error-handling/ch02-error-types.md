# Chapter 8.2 — Designing Error Types: Libraries vs Applications

> **Where this sits:** Part VIII · Error Handling · chapter 2 of 4
> **Prerequisites:** Chapter 8.1; Chapters 2.4–2.5 (the frame-header parser); Project L1 (`logstat`). Part VI (traits)
> helps but is not required.
> **After this chapter you can:** design an error type as an API, with variants for programs and messages for people;
> implement `std::error::Error` by hand and with `thiserror`; build application errors with `anyhow` and context;
> print a cause chain correctly; measure what `Box<dyn Error>`, `anyhow`, and large error enums cost (allocations,
> sizes, verified assembly); and redesign the `String` errors in Chapter 2.4's parser and in `logstat`.

---

## Pass 1 · User level — *An error type is an API*

### 1. Problem

Chapter 2.4's frame-header parser and Project L1's `logstat` both used `String` errors. That was deliberate: Part II
hadn't introduced traits, and a `String` is the simplest thing that can carry a message. Both chapters promised this
redesign. Here's why it's needed.

The parser's caller has to make a **decision** on every error: if the header is incomplete, wait for more bytes from the
socket; if the magic number is wrong, close the connection. With a `String`, the only way to decide is to read the
prose. From listing `ch02-01-header-errors.rs`, the "before" version:

```rust,ignore
pub fn parse_header(buf: &[u8]) -> Result<u32, String> {
    match buf {
        [0xCA, 0xFE, _, _, l0, l1, l2, l3, ..] => Ok(u32::from_be_bytes([*l0, *l1, *l2, *l3])),
        [a, b, _, _, _, _, _, _, ..] => Err(format!("bad magic {a:#04x} {b:#04x}")),
        short => Err(format!("need 8 header bytes, got {}", short.len())),
    }
}

/// The caller's only way to decide "wait for more bytes" vs "drop the connection".
pub fn should_wait(e: &str) -> bool {
    e.contains("need")
}
```

The listing also has a copy of the parser in which one message was reworded to `"short read: {} of 8 header bytes"`, the
kind of change nobody reviews carefully. On the same 4-byte input (verified):

```text
before:          should_wait = true
before, reworded: should_wait = false
```

A wording change silently changed the program's behavior, and nothing failed to compile. `logstat` has the other
classic problem: `io::Error::new(e.kind(), format!("{path}: {e}"))` keeps the error's *kind* but flattens the original
error into text, so the cause is no longer an error value anyone can inspect.

This chapter is about designing error types that programs can act on and people can read, and about the one big fork
in the road: **library** errors versus **application** errors.

### 2. Mental model

**An error type has two audiences.**

| Audience | Needs | Served by |
|---|---|---|
| **Programs** (the caller's code) | To *decide*: retry? wait? return 404 or 500? exit with 2 or 1? | The **variants** (and their fields) |
| **People** (users, operators) | To *understand*: what failed, doing what, and why | `Display` of each layer, joined through `source()` |
| **Developers** debugging | Everything | `Debug`, and optionally a backtrace |

**The `Error` trait** [LIB]:

```rust,ignore
pub trait Error: Debug + Display {
    fn source(&self) -> Option<&(dyn Error + 'static)> { None }   // the underlying cause, if any
    // (`description` and `cause` are deprecated; `provide` is unstable)
}
```

**Errors form a chain.** Each layer describes what *it* was doing and points to the cause:

```text
 LogstatError::Open { path: "/var/log/x.log", .. }    Display: "cannot open /var/log/x.log"
    └─ source() ─► io::Error (NotFound)              Display: "No such file or directory (os error 2)"
                      └─ source() ─► None

 a reporter walks the chain:  "logstat: cannot open /var/log/x.log: No such file or directory (os error 2)"
```

**The chain rule:** a layer's `Display` says what that layer was doing and **does not repeat its cause**. The cause is
reached through `source()`, and a reporter (a CLI's `main`, a log formatter, anyhow's `{:#}`) prints the chain. Print a
cause in `Display` *and* return it from `source()`, and every report shows it twice (this chapter's debugging exercise).

**Library vs application.** The most useful distinction in Rust error design:

| | Library error | Application error |
|---|---|---|
| Who handles it | Callers you don't know yet | Your own `main`, request handler, or job runner |
| Shape | A precise enum: one variant per failure mode a caller might handle differently | One opaque type that can hold anything, plus context |
| Typical tool | Hand-written impls or `thiserror` (generates them) | `anyhow::Error` (or `Box<dyn Error + Send + Sync>`) |
| Callers match on it? | Yes: that's its purpose | Rarely: it's reported, or mapped once at a boundary |
| Context lives in | Variant fields (`Open { path, source }`) | `.context("reading limits.conf")` calls |
| Evolution | `#[non_exhaustive]`; adding a variant must not break callers | Free to change |

The same program often has both: a `ledger` library crate with a `LedgerError` enum, used by a service binary whose
handlers return `anyhow::Result` internally and map to HTTP at the edge (Chapter 8.4).

### 3. Rust code

**The header parser, redesigned.** The error becomes data, and each variant is a distinct decision for the caller
(excerpt of listing `ch02-01-header-errors.rs`, verified):

```rust,ignore
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
```

The caller dispatches on the variant, never on text:

```rust,ignore
match parse_header(input) {
    Ok((h, body)) => println!("frame   v{} body_len={} body={:?}", h.version, h.body_len, std::str::from_utf8(body)),
    Err(e) if e.is_incomplete() => println!("wait    {e}"),
    Err(e) => println!("close   {e}"),
}
```

```text
frame   v1 body_len=5 body=Ok("hello")
wait    incomplete header: need 8 bytes, got 4
close   bad magic 0x47 0x45
close   unsupported protocol version 2
close   declared body of 4294967295 bytes exceeds 1048576
```

Two new checks came almost for free once errors had names: a version check and a **body-length limit**. The last input
declares a 4 GiB body. The `String` version would have accepted it and let the caller try to buffer 4 GiB, which is a
denial-of-service bug. `split_first_chunk::<8>()` [VERSION] (stable since 1.77) returns the first 8 bytes as a
`&[u8; 8]`, so the destructuring `let [m0, m1, ...] = *head` can't fail.

**A library error with `thiserror`.** Writing `Display` and `Error` by hand is mechanical. The `thiserror` derive macro
writes them for you (listing `ch02-02-thiserror.rs`, verified; `thiserror` 2.x on the Playground):

```rust,ignore
/// A library error: every variant is something a caller might handle differently.
#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    #[error("account {0} not found")]
    AccountNotFound(u64),
    #[error("insufficient funds: balance {balance}, requested {amount}")]
    InsufficientFunds { balance: i64, amount: i64 },
    #[error("ledger storage unavailable")]
    Storage(#[from] io::Error),
    #[error("corrupt ledger record at line {line}")]
    Corrupt {
        line: usize,
        #[source]
        source: ParseIntError,
    },
}

fn parse_balances(text: &str) -> Result<Vec<i64>, LedgerError> {
    text.lines()
        .enumerate()
        .map(|(i, l)| l.trim().parse::<i64>().map_err(|source| LedgerError::Corrupt { line: i + 1, source }))
        .collect() // Iterator<Item = Result<T, E>> collects into Result<Vec<T>, E>: stops at the first Err
}

fn load(path: &str) -> Result<Vec<i64>, LedgerError> {
    let text = std::fs::read_to_string(path)?; // io::Error -> LedgerError::Storage, via #[from]
    parse_balances(&text)
}

/// Print an error and its chain of causes, each exactly once.
fn report(e: &(dyn Error + 'static)) {
    println!("error: {e}");
    let mut cause = e.source();
    while let Some(c) = cause {
        println!("  caused by: {c}");
        cause = c.source();
    }
}
```

```text
Ok(300)
error: insufficient funds: balance 120, requested 200
  variant decides: retryable=false
error: account 9 not found
  variant decides: retryable=false
error: corrupt ledger record at line 2
  caused by: invalid digit found in string
  variant decides: retryable=false
error: ledger storage unavailable
  caused by: No such file or directory (os error 2)
  variant decides: retryable=false
```

`#[error("...")]` generates `Display` (fields are interpolated by name or position). `#[from]` generates a `From` impl
**and** marks the field as the `source()`. `#[source]` marks the source without generating `From`: `Corrupt` needs a
line number that a bare `ParseIntError` can't supply, so it's built explicitly with `map_err`. Note that no message
repeats its cause. The chain rule is followed, and `report` prints each cause once.

**An application error with `anyhow`.** Application code wants something different: every failure should carry
context about what the program was doing, and nobody is going to match on variants (listing `ch02-03-anyhow.rs`):

```rust
use anyhow::{Context, Result, bail};
use std::io;
use std::path::Path;

fn parse_limit(text: &str) -> Result<u64> {
    let n: u64 = text.trim().parse().context("limit is not a whole number")?;
    if n == 0 {
        bail!("limit must be positive");
    }
    Ok(n)
}

/// Application code: every failure gets context, nobody matches on variants.
fn load_limit(path: &Path) -> Result<u64> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {name}"))?;
    parse_limit(&text).with_context(|| format!("parsing {name}"))
}

fn main() -> Result<()> {
    let dir = tempfile::tempdir()?;
    for (name, body) in [("good.conf", "500\n"), ("bad.conf", "5OO\n"), ("zero.conf", "0\n")] {
        std::fs::write(dir.path().join(name), body)?;
    }
    for name in ["good.conf", "bad.conf", "zero.conf", "missing.conf"] {
        match load_limit(&dir.path().join(name)) {
            Ok(n) => println!("{name}: limit {n}"),
            Err(e) => {
                println!("{name}:");
                println!("  {{}}   {e}");
                println!("  {{:#}}  {e:#}");
                println!("  chain length {}; root cause is io::Error? {}", e.chain().count(), e.root_cause().is::<io::Error>());
                if let Some(io) = e.downcast_ref::<io::Error>() {
                    println!("  downcast_ref::<io::Error>() -> kind {:?}", io.kind());
                }
            }
        }
    }
    let e = load_limit(&dir.path().join("bad.conf")).unwrap_err();
    println!("--- {{:?}} ---\n{e:?}");
    Ok(())
}
```

```text
good.conf: limit 500
bad.conf:
  {}   parsing bad.conf
  {:#}  parsing bad.conf: limit is not a whole number: invalid digit found in string
  chain length 3; root cause is io::Error? false
zero.conf:
  {}   parsing zero.conf
  {:#}  parsing zero.conf: limit must be positive
  chain length 2; root cause is io::Error? false
missing.conf:
  {}   reading missing.conf
  {:#}  reading missing.conf: No such file or directory (os error 2)
  chain length 2; root cause is io::Error? true
  downcast_ref::<io::Error>() -> kind NotFound
--- {:?} ---
parsing bad.conf

Caused by:
    0: limit is not a whole number
    1: invalid digit found in string
```

Three formats, three audiences: `{}` is the outermost context only (for a terse UI), `{:#}` joins the chain on one line
(for logs), and `{:?}` is the multi-line report. `downcast_ref` recovers the concrete `io::Error` when a program *does*
need to branch. Because `anyhow::Error`'s `Debug` prints the chain, `fn main() -> anyhow::Result<()>` gives a readable
report where Chapter 8.1's `ParseIntError` gave a struct dump (listing `ch02-04-anyhow-main.rs`, stderr):

```text
Error: loading rate limits from /etc/meridian/limits.conf

Caused by:
    No such file or directory (os error 2)
```

---

## Pass 2 · Systems level — *What the tools generate, and what errors cost*

### 4. Under the hood

**`thiserror` writes the impls you would have written.** [LIB] It's a procedural macro (Chapter 18.2 shows where
expansion happens in rustc). The Playground's macro expansion of listing `ch02-02-thiserror.rs` (nightly, trimmed) shows
its output:

```rust,ignore
impl ::thiserror::__private20::Error for LedgerError {           // a re-export of std::error::Error
    fn source(&self) -> ::core::option::Option<&(dyn ::thiserror::__private20::Error + 'static)> {
        use ::thiserror::__private20::AsDynError as _;
        match self {
            LedgerError::AccountNotFound { .. } => ::core::option::Option::None,
            LedgerError::InsufficientFunds { .. } => ::core::option::Option::None,
            LedgerError::Storage { 0: source, .. } => ::core::option::Option::Some(source.as_dyn_error()),
            LedgerError::Corrupt { source: source, .. } => ::core::option::Option::Some(source.as_dyn_error()),
        }
    }
}

impl ::core::fmt::Display for LedgerError {
    fn fmt(&self, __formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        match self {
            LedgerError::Storage(_0) => __formatter.write_str("ledger storage unavailable"),
            LedgerError::Corrupt { line, source } => match (line.as_display(),) {
                (__display_line,) => __formatter.write_fmt(format_args!("corrupt ledger record at line {0}", __display_line)),
            },
            // ... one arm per variant ...
        }
    }
}

impl ::core::convert::From<io::Error> for LedgerError {
    fn from(source: io::Error) -> Self { LedgerError::Storage { 0: source } }
}
```

So `thiserror` has **no run-time component** and doesn't appear in your public API: callers see an ordinary enum
implementing `std::error::Error`. Replacing it with hand-written impls later is not a breaking change.

**`anyhow::Error` is a thin pointer to a boxed error with a hand-made vtable.** [LIB] Converting an error into
`anyhow::Error` allocates one heap block holding a vtable pointer, an optional backtrace, and the error itself. The
handle is one pointer (8 bytes, Chapter 8.1). `.context(c)` wraps the error in a new allocation that holds `c` and the
inner error, and presents the inner one as its `source()`. That's where the chain in the output came from.
`downcast_ref::<T>()` compares `TypeId`s through the vtable, so it finds only the exact concrete type, with no
subtyping.

**`Box<dyn Error + Send + Sync>`** [LIB] is the std-only equivalent. There's a blanket
`impl<E: Error + Send + Sync + 'a> From<E> for Box<dyn Error + Send + Sync + 'a>`, plus `From<&str>` and
`From<String>`. That's why `?` works on any error and why `.ok_or("no such payment")?` compiles in the Part VIII review
PR. It's a fat pointer (16 bytes), it has no context helpers, and its `Debug` doesn't print the chain.

> **Why not always `Box<dyn Error>`?** In a library, it throws away the one thing callers need, the ability to decide.
> A caller holding `Box<dyn Error>` must `downcast_ref` to every concrete type it can think of, and your internal types
> become an undocumented API. Also watch the bounds: `Box<dyn Error>` without `+ Send + Sync` can't cross threads or be
> held across an `.await` in a multi-threaded Tokio runtime (Part XIII).

**Backtraces are opt-in.** [LIB] [VERSION] `std::backtrace::Backtrace` (stable since 1.65) has `capture()`, which
records a trace only if `RUST_LIB_BACKTRACE` or `RUST_BACKTRACE` enables it, and `force_capture()`, which always does.
Listing `ch02-08-backtrace.rs` stores one in an error:

```text
invariant violated: debits != credits; capture() status: Disabled
force_capture: 18 frame lines, mentions main: true
capture took 29.8µs, first render took 4.568397ms (one run, noisy)
```

Capturing walks the stack and stores raw addresses (tens of microseconds here). **Symbolization**, turning addresses
into function names and lines by reading debug info, happens lazily on first `Display` and took milliseconds. Hence the
design: capture only when enabled, and only for errors that indicate bugs (an invariant violation), not for expected
ones (a declined card). `anyhow` captures automatically when the environment variables enable it. [VERSION] std's
generic way to attach a backtrace to *any* error (`Error::provide`) is still unstable.

### 5. Memory

**What each representation costs** (listing `ch02-05-error-costs.rs`, debug and release gave identical output):

```text
sizes: Result<u32, ConfigError>=8  Result<u32, Box<dyn Error+Send+Sync>>=16  anyhow::Result<u32>=16
sizes: Result<u64, BigError>=520  Result<u64, Box<BigError>>=16
 8080: enum 0 alloc/0 free   Box<dyn Error> 0/0   anyhow 0/0
  80x: enum 0 alloc/0 free   Box<dyn Error> 1/1   anyhow 1/1
```

(Counts from the counting allocator of Part III, around `parse_*(input)` followed by dropping the result.)

- **Success is free** in all three: no allocation.
- **Failure** costs one allocation and one free for `Box<dyn Error>` and `anyhow`, and nothing for the enum. At 1% error
  rate and 400K requests/s that's 4,000 allocations per second (arithmetic): irrelevant. On a hot parser rejecting 30%
  of its input, it's measurable, and a plain enum is the better choice.
- **Size is paid on every return.** `BigError` carries a 512-byte buffer inline, so *every* `Result<u64, BigError>` is
  520 bytes, including `Ok(5)`. Boxing the error makes it 16.

Clippy's `result_large_err` lint flags this pattern [LIB]: it warns when a function returns a `Result` whose `Err` type
is large (the threshold defaults to 128 bytes at the time of writing, configurable as `large-error-threshold`). *Not
verified here: requires `cargo clippy`.* The usual fixes are boxing the error (or its large variant) or moving bulky
context out of the error (log it where it's known, keep an ID in the error).

### 6. CPU / OS

**Where a large error goes: memory, not registers.** Listing `ch02-06-large-error-asm.rs` defines the same two-layer
function twice, `outer(x) { let v = check(x)?; Ok(v + 1) }`, once with `BigError` and once with `Box<BigError>`. Release
assembly, rustc 1.98.1 (trimmed; `@GOTPCREL` call syntax simplified):

```text
playground::outer_boxed:                  ; Result<u64, Box<BigError>>: 16 bytes, returned in rax:rdx
	push	rax
	call	check_boxed
	lea	rcx, [rdx + 1]            ; v + 1, computed speculatively
	test	al, 1                     ; `?`: Err?
	cmove	rdx, rcx                  ; Ok → payload = v + 1; Err → keep the Box pointer
	and	eax, 1
	pop	rcx
	ret

playground::outer_big:                    ; Result<u64, BigError>: 520 bytes, returned through memory
	push	rbp / r14 / rbx
	sub	rsp, 528                  ; a 528-byte frame to receive check_big's result
	mov	rbx, rdi                  ; rdi = hidden pointer to OUR caller's 520-byte return slot
	lea	rdi, [rsp + 8]
	call	check_big                 ; check_big writes its result into our frame
	cmp	dword ptr [rsp + 8], 1    ; `?`: Err?
	jne	.LBB3_2
	mov	ebp, dword ptr [rsp + 12] ; Err: copy the error into our caller's slot...
	mov	r14, qword ptr [rsp + 16]
	lea	rsi, [rsp + 24]
	lea	rdi, [rbx + 16]
	mov	edx, 504
	call	memcpy                    ; ...504 bytes of it via memcpy
	mov	dword ptr [rbx + 4], ebp
	mov	eax, 1
	jmp	.LBB3_3
.LBB3_2:                                  ; Ok: load v from memory, add 1
	mov	r14, qword ptr [rsp + 16]
	inc	r14
	xor	eax, eax
.LBB3_3:
	mov	qword ptr [rbx + 8], r14  ; store into the caller's slot (memory)
	mov	dword ptr [rbx], eax      ; store the tag
	(epilogue) / ret
```

What the machine does differently:

- **Return convention.** [CPU] [RUSTC] A 16-byte `Result` comes back in two registers. A 520-byte `Result` can't, so the
  caller passes a hidden pointer ("sret") to a slot it reserved, and the callee writes there. Even the `Ok` path of
  `outer_big` now loads and stores through memory.
- **Propagation copies.** On `Err`, `?` moves the error from the callee's slot to our caller's slot: a 504-byte
  `memcpy` plus a few loose fields, **at every layer** the error passes through. With the box, propagation copies one
  pointer (`cmove`, no branch at all).
- **Stack.** Each layer reserves a result-sized buffer (528 bytes here). Deep call chains with large errors inflate
  stack usage, which matters on small thread stacks (Chapter 2.4's 2 MiB spawned threads).

Boxing trades these for an **allocation on the failure path only**, which §5 counted. When failures are rare, that
trade almost always wins. When failures are common, keep errors small instead: a code, a few integers, and a
`&'static str`.

---

## Pass 3 · Architect level — *Choosing a representation*

### 7. Trade-offs

**Six ways to represent errors:**

| Representation | Caller can branch? | Context | Cost on failure | API evolution | Use for |
|---|---|---|---|---|---|
| `String` | Only by parsing prose | Ad hoc, in the text | 1+ allocations | Every rewording is a behavior change | Prototypes only |
| Hand-written enum | Yes, exhaustively | Fields | None beyond size | Add variants with `#[non_exhaustive]` | Small libraries, `no_std`, hot paths |
| `thiserror` enum | Same as above | Fields + `#[source]` | Same as above | Same as above | Most libraries |
| Opaque struct + `kind()` (the `io::Error` pattern) | Via a `Kind` enum | Private fields | Often a box | Best: internals can change freely | Large, long-lived library APIs |
| `Box<dyn Error + Send + Sync>` | Only by `downcast_ref` | None built in | 1 allocation | Callers depend on internals | Quick apps, test helpers |
| `anyhow::Error` | Only by `downcast_ref` | `.context()`, chain printing, backtraces | 1 allocation per layer of context | N/A (applications) | Application code, `main`, jobs, CLIs |

**Granularity.** One crate-wide `Error` enum with 30 variants is the Rust equivalent of `throws Exception`: every
function appears to return every error, and callers can't tell which are possible. Prefer **one error type per
operation family**: `HeaderError` for parsing a header, `LedgerError` for ledger operations. A function's error type
should list what *it* can fail with.

**`#[non_exhaustive]`** [LANG] on a public enum forces *downstream* crates to include a wildcard arm, so adding a variant
is not a breaking change. Inside the defining crate it has no effect, so your own mapping code (Chapter 8.4's
`http_status`) still gets exhaustiveness errors when a variant is added. That's the combination you want.

**Don't leak dependency types.** A public variant `Db(rusqlite::Error)` makes your API's semver depend on `rusqlite`'s.
Wrap it (`Storage { source: Box<dyn Error + Send + Sync> }`, or a private field behind an opaque type) and expose the
kind, not the type.

**Display conventions** (from the Rust API Guidelines): lowercase, no trailing punctuation, concise, and no cause in
the message. Errors should be `Send + Sync + 'static` so they can cross threads, be held across `.await`, and be
downcast.

**Where the library/application line falls.** Inside one service binary, domain modules often deserve library-style
enums (the payments module's `PaymentError`, Chapter 8.4), because the HTTP boundary *branches* on them. Plumbing code
(startup, config, migrations) can use `anyhow`. The question to ask is "will some code decide something based on which
error this is?", not "is this a library crate?".

### 8. Java comparison

| Java / Go | Rust | Notes |
|---|---|---|
| Exception class hierarchy | Enum (closed set of variants) | Java's hierarchy is open: anyone can subclass, and `catch (IOException e)` catches subclasses. A Rust enum is closed, and `match` is exhaustive. |
| `Throwable.getCause()` | `Error::source()` | Same idea. Java prints causes in `printStackTrace`; Rust needs a reporter (anyhow's `{:?}`, or a loop). |
| `new ServiceException("loading limits", e)` | `.context("loading limits")` (anyhow), or a variant with `#[source]` | Wrapping with a cause. |
| `throws Exception` | `anyhow::Result<T>` | "Anything can go wrong; I'll report it." Fine for applications. |
| Stack trace captured in every `Throwable` constructor | `Backtrace::capture()`, opt-in via env vars | Java pays for a trace on every exception (unless `writableStackTrace=false`). Rust pays only when asked. |
| `catch (FileNotFoundException e)` | `match` on a variant, or `downcast_ref::<io::Error>()` + `kind()` | |
| Go `fmt.Errorf("reading %s: %w", name, err)` | `.with_context(\|\| format!("reading {name}"))` | Go's `%w` wrapping is the closest analog to anyhow's context. |
| Go `errors.Is(err, fs.ErrNotExist)` / `errors.As(err, &target)` | `e.kind() == NotFound` / `e.downcast_ref::<T>()` | Go's sentinel errors ≈ unit variants. |

> **Analogy limit.** "A Rust error enum is an exception hierarchy" fails in both directions. Enums are **closed**: a
> caller can match every case and the compiler checks it, which a Java hierarchy can't offer. But there's no
> subtyping: you can't catch "any `IoError`" across several enums unless someone wrote a `kind()` or a common variant
> for it. And `anyhow`'s `downcast_ref` is not `catch` by type: it only finds the exact concrete type in the chain.

### 9. Production scenario

**`logstat`'s errors, redesigned** (the Project L1 promise). Before, from `listings/part-02/project-01-logstat.rs`:

```rust,ignore
pub fn parse(argv: &[String]) -> Result<Command, String> { ... }       // usage errors: prose
    return Err("--top needs a value".to_string());
    .map_err(|_| format!("--top expects a number, got {value:?}"))?;

let file = File::open(path).map_err(|e| io::Error::new(e.kind(), format!("{path}: {e}")))?;  // cause flattened

Err(msg) => { eprint!("logstat: {msg}\n{USAGE}"); return ExitCode::from(2); }   // exit code chosen by the
Err(e) => { eprintln!("logstat: {e}"); return ExitCode::from(1); }              // call site, not the error
```

After (excerpt of listing `ch02-07-logstat-errors.rs`, verified, with tests):

```rust,ignore
#[derive(Debug, PartialEq)]
pub enum ArgsError {
    MissingValue { flag: &'static str },
    NotANumber { flag: &'static str, value: String },
    UnknownOption(String),
}

#[derive(Debug)]
pub enum LogstatError {
    Usage(ArgsError),
    Open { path: String, source: io::Error },
    Read { path: String, source: io::Error },
    Write(io::Error),
}

impl Error for LogstatError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Usage(e) => e.source(), // transparent: Display already printed `e`
            Self::Open { source, .. } | Self::Read { source, .. } | Self::Write(source) => Some(source),
        }
    }
}

impl LogstatError {
    /// The process contract: 2 = you called me wrong, 1 = I failed at run time.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::Open { .. } | Self::Read { .. } | Self::Write(_) => 1,
        }
    }

    /// `logstat big.log | head -3` closes stdout early. That is the reader's choice, not our failure.
    pub fn is_benign(&self) -> bool {
        matches!(self, Self::Write(e) if e.kind() == io::ErrorKind::BrokenPipe)
    }
}

fn open(path: &str) -> Result<std::fs::File, LogstatError> {
    std::fs::File::open(path).map_err(|source| LogstatError::Open { path: path.to_string(), source })
}
```

Output of the listing's five scenarios (a real CLI writes these lines to stderr):

```text
logstat: --top needs a value   (exit 2)
logstat: --top expects a number, got "ten"   (exit 2)
logstat: unknown option "--verbose"   (exit 2)
logstat: cannot open /var/log/missing-access.log: No such file or directory (os error 2)   (exit 1)
["--top", "3"] /dev/null: success (closed pipe is not an error)
```

What changed, and why each matters:

- **Tests assert on values**, not strings: `assert_eq!(parse_top(&["--top"]), Err(ArgsError::MissingValue { flag: "--top" }))`.
  Rewording a message can't break a test or a caller.
- **The exit-code contract lives on the error** (`exit_code()`), not in whichever `match` arm happens to print it. Add a
  variant and the compiler makes you choose its exit code.
- **The cause stays an error value.** `Open { path, source }` keeps the `io::Error`, so a caller could still check
  `kind() == PermissionDenied`, and the one-line rendering is produced by walking the chain.
- **Benign failures are classified once** (`is_benign`), instead of a `BrokenPipe` check buried in `main`.
- `LogstatError` is library-style (an enum), not `anyhow`, because `main` **branches** on it: exit codes and benign
  cases. A tool that only ever printed its errors could use `anyhow` in `main` and lose nothing.

### 10. Failure scenario

**The reconnect storm caused by a reworded message.** Meridian's Rust ingest path for the market-data fan-out
(Chapter 1.1's C++ system, being rewritten) used the Chapter 2.5 parser with `String` errors, and a read loop that
decided what to do by searching the message:

```rust,ignore
Err(e) if e.contains("need") => continue,   // incomplete: read more bytes
Err(e) => { conn.close(); return Err(e); }  // protocol violation: drop the feed
```

A tidy-up PR reworded parser messages for consistency, `"need 8 header bytes, got 5"` becoming `"short read: 5 of 8
header bytes"`. Tests passed: they checked that parsing a short buffer returned `Err`, not what the caller did with it.
In production, TCP delivers frames split across reads all the time (Chapter 2.4's design exercise), and each split
header now matched the second arm. Every feed connection dropped within seconds of its first split frame, reconnected,
and dropped again. The reconnect storm tripped the exchanges' connection rate limits, and the fan-out served stale prices
until the PR was reverted, about 25 minutes later.

`before::should_wait` in §1 reproduces the mechanism in two lines of verified output. The fix is §3's `HeaderError`:
`Incomplete` is a **variant**, so the decision can't depend on wording. Adding a variant, or renaming one, is a compile
error at the dispatch site, not an outage. The team added two rules to its review checklist: *no `String` errors in
library code*, and *no `.contains(` or `==` on error messages anywhere*. (Tests may assert on `Display` output only in
tests *of* the `Display` output.)

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part VIII).*

1. What are the two audiences of an error type, and which parts of the type serve each?
2. State the chain rule for `Display` and `source()`. What goes wrong if a layer violates it?
3. When would you use `thiserror` and when `anyhow`? Is the answer "library crate vs binary crate"?
4. What code does `#[derive(thiserror::Error)]` generate for a variant marked `#[from]`? Does `thiserror` appear in
   your public API?
5. How is `anyhow::Error` 8 bytes when `Box<dyn Error + Send + Sync>` is 16? What does converting an error into either
   cost?
6. Why can a 520-byte `Result` slow down the *success* path? Describe what the verified assembly shows.
7. What does `#[non_exhaustive]` do inside the defining crate, and outside it? Why is that the right combination for
   error enums?
8. `Backtrace::capture()` returned `Disabled`. Why is that the default, and where in a system should you force a
   capture?

### 12. Exercises

- **Beginner.** Convert Chapter 8.1's `ConfigError` to `thiserror`. Check that the output of listing
  `ch01-02-question-mark.rs` is unchanged.
- **Intermediate.** Write `fn load_accounts(path) -> Result<Vec<Account>, LoadError>` where each line is
  `id,balance`. Errors must say which line failed and why, and keep the `ParseIntError` as `source()`. Print a failure
  with a chain-walking reporter and with `anyhow`'s `{:#}` (convert at the call site).
- **Advanced.** Implement the `io::Error` pattern: a public `struct Error { kind: ErrorKind, inner: Box<Inner> }` with a
  public `#[non_exhaustive] enum ErrorKind` and private `Inner`. What can you change later without a semver-major
  release that you couldn't with a public enum?
- **Systems.** Extend listing `ch02-05-error-costs.rs` to measure `anyhow` with two layers of `.context()`. Predict the
  allocation count first. Then emit release assembly for `outer_big` with a 64-byte error and find the size at which
  rustc switches from registers to a hidden return pointer.
- **Architecture.** Pick a library you depend on and read its error type. Classify it (enum, opaque, boxed, stringly).
  Can your code make every decision it needs from the type alone? What would you change?

### 13. Debugging exercise

A teammate converts `logstat` to `thiserror` (listing `ch02-09-doubled-cause.rs`):

```rust,ignore
#[derive(Debug, thiserror::Error)]
pub enum ArgsError {
    #[error("--top needs a value")]
    MissingValue,
}

#[derive(Debug, thiserror::Error)]
pub enum LogstatError {
    #[error("{0}")]
    Usage(#[from] ArgsError),
    #[error("cannot open {path}")]
    Open { path: String, source: std::io::Error },
}
```

The chain-walking `render` function from §9 now prints:

```text
logstat: --top needs a value: --top needs a value
logstat: cannot open access.log: entity not found
```

1. Why is the first message doubled? Name the two mechanisms that each print `ArgsError`.
2. Give two different fixes (one changes the `#[error]` attribute, one uses a different `thiserror` attribute). What
   does each print?
3. The second line has no doubling, yet `Open` has no `#[source]` attribute. Why is `source` still the error's source?
   (Hint: read the field's name.)
4. The `io::Error` prints "entity not found" here but "No such file or directory (os error 2)" in §9. Why the
   difference? Which one would you rather have in a log?

### 14. Design exercise

**Ferrite's error type.** Ferrite v1 (Project L4) defines `trait KvStore { fn get(&self, key: &[u8]) ->
Option<Vec<u8>>; ... }`: an in-memory store can't fail. Ferrite v3 (Project L7) adds a write-ahead log and segment
files, and now `put` can fail (disk full, fsync error), `get` can fail (corrupt segment, checksum mismatch), and startup
recovery can fail in several ways.

- Design `FerriteError`. Which variants does a *caller* branch on (retry? read-only mode? crash?), and which exist only
  for reporting?
- How does the trait's signature change? Can v1's in-memory store implement the new trait without inventing errors?
- Which errors should include a backtrace, and which never should?
- The network protocol replies `-ERR <message>`. What should a client receive for each variant, and what must it never
  receive?
