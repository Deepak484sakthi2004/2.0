# Chapter 9.2 — String and Text Encoding

> **Where this sits:** Part IX · Collections and Memory · chapter 2 of 5
> **Prerequisites:** Chapter 3.4 (UTF-8, `&str` vs `String`, char boundaries, `Cow`), Chapter 9.1 (`Vec` growth).
> **After this chapter you can:** build text without hidden allocations; predict what `format!`, `+`, `write!`, and
> `join` cost; convert between UTF-8 and UTF-16 at Windows and Java boundaries without corrupting data; explain why
> case mapping allocates and when it can't; and choose among `String`, `Box<str>`, `Arc<str>`, and small-string types.

---

## Pass 1 · User level — *A `Vec<u8>` with a promise*

### 1. Problem

Chapter 3.4 covered what a `String` *is*: owned UTF-8 bytes, sliced only at char boundaries, and borrowed as `&str` or
`Cow<str>` when you can avoid owning. This chapter is about what strings *cost* in a server: building them (log lines,
CSV rows, cache keys), converting them at the edges (Windows APIs, Java via JNI or FFM, bytes from the network), and
normalizing them (case-insensitive headers, usernames). These are the places where a Java habit, carried over
unexamined, turns into an allocation per request or a quiet data-corruption bug.

### 2. Mental model

```text
 String  = Vec<u8> + invariant "the bytes are valid UTF-8"
           (cap, ptr, len), 24 bytes; growth is Vec<u8>'s: 0 → 8 → 16 → 32 → ...

 Building text:     one growing buffer, written into          ← cheap
                    vs. many temporary Strings, glued         ← one allocation per temporary

 At the edges:      bytes ──validate──► &str            (borrow, no copy, can fail)
                    bytes ──lossy─────► Cow<str>         (borrow if valid, allocate + U+FFFD if not)
                    UTF-16 ─decode────► String           (always allocates, can fail on lone surrogates)
```

Two rules follow:

- **Write into buffers, don't concatenate temporaries.** Every `format!` returns a new `String`, so every `format!`
  is at least one allocation.
- **Decide at every boundary what happens to invalid text.** Rust won't let you ignore it: the conversion either
  returns `Result`, or its name says `lossy`.

### 3. Rust code

**Six ways to build 10,000 CSV rows** (listing `ch02-01-string-building.rs`, release, counting allocator; one run on a
shared machine, so times are indicative):

```rust,ignore
// 2. The accidentally quadratic version: format! re-copies the whole prefix every time.
let mut s = String::new();
for r in &rows {
    s = format!("{s}{},{}\n", r.id, r.price);
}

// 4. write! straight into the buffer: no temporaries, amortized growth only.
let mut s = String::new();
for r in &rows {
    writeln!(s, "{},{}", r.id, r.price).unwrap();
}
```

```text
s = s + &format!(..)                19115 allocs    98090 bytes  834.123µs
s = format!("{s}..")                19999 allocs    98090 bytes  80.6078ms
s.push_str(&format!(..))            19115 allocs    98090 bytes  752.243µs
writeln!(s, ..)                        15 allocs    98090 bytes  422.181µs
with_capacity + writeln!                1 allocs    98090 bytes  399.752µs
collect::<Vec<String>>().join       19002 allocs    98089 bytes  1.041684ms
```

Four lessons are in that table:

1. **`s = s + &x` is not the Java trap.** `impl Add<&str> for String` takes the left `String` *by value* and appends
   to its buffer. No copy of the prefix happens, and the 15 extra allocations are the buffer's normal doubling. The
   19,100 other allocations come from the `format!` temporaries.
2. **`s = format!("{s}...")` is the trap.** It builds a brand-new string containing all of `s` every iteration: O(n²)
   bytes copied, about 100× slower here, and it gets worse linearly with n.
3. **`write!`/`writeln!` into a `String` allocates only when the buffer grows**: 15 times for 98 KB, or once if you
   presize. `String` implements `std::fmt::Write`, so the formatting machinery writes straight into it. For a `String`
   the `Result` is always `Ok`; the `unwrap` is a formality.
4. **`join` is efficient at gluing and doesn't fix the pieces.** It sums the lengths and allocates the result once, but
   the 10,000 pieces were already 19,000 allocations.

Why ~1.9 allocations per `format!` rather than 1? [LIB] `format!` sizes its initial buffer from the format string's
literal pieces (`Arguments::estimated_capacity`). When the string *starts* with a placeholder and the literals are
short, as in `"{},{}\n"`, the estimate is 0, so the result grows 0 → 8 → 16. Rows of at most 8 bytes (ids below
900) needed one allocation, and the 9,100 longer rows needed two: 900 + 18,200 = 19,100. It's a heuristic, and it's
another reason to write into a buffer you control.

**What's free, and what isn't:**

| Expression | Allocates? | Notes |
|---|---|---|
| `println!("{x}")`, `write!(w, ...)` | no String built | formats straight into the writer (stdout is locked per call) |
| `format_args!(...)` | no | produces `fmt::Arguments`, a borrowed description of the formatting; pass it to `write_fmt` |
| `format!(...)`, `x.to_string()` | yes, ≥ 1 | a new `String` every time |
| `concat!("a", "b")` | no | compile-time, literals only |
| `[a, b, c].concat()`, `.join(sep)` | 1 for the result | exact size precomputed |
| `s + &t`, `s += &t`, `push_str` | only when `s` grows | reuses `s`'s buffer |
| `String::with_capacity(n)` | 1 | then no growth until n bytes |

---

## Pass 2 · Systems level — *Formatting machinery, encodings, and case tables*

### 4. Under the hood

**How `format!` works.** [RUSTC] `format_args!` is a compiler built-in. The format string is parsed **at compile time**:
a typo in a placeholder or a missing argument is a compile error, and the literal pieces become static data. What it
produces at run time is an `fmt::Arguments` value: the static pieces plus, for each argument, a reference to it and a
pointer to its formatting function (`<u32 as Display>::fmt`, and so on). The formatting itself runs through
`&mut dyn fmt::Write`, one indirect call per argument and per piece.

Two consequences:

- **Formatting doesn't monomorphize per call site.** That keeps binary size down (every `println!` shares one
  formatting engine) at the cost of indirect calls. It's fast enough for logging and error messages, but it's not the
  fastest way to turn an integer into digits. For a hot serialization path, the ecosystem crates `itoa` and `ryu`
  ([LIB], used inside `serde_json`) write integers and floats directly. Measure before switching.
- **Java parses at run time.** `String.format("%d,%d%n", a, b)` parses its pattern on every call, and a bad pattern is
  a runtime `IllegalFormatException`. Rust moves both the parsing and the error to compile time.

**Why `write!` returns `Result`.** `fmt::Write::write_str` can fail for writers that can fail (a fixed-size buffer, say).
`String`'s implementation never does. For `io::Write` (files and sockets) the error is real, and Chapter 3.5's export
incident is about exactly that.

**UTF-16 conversions** (listing `ch02-02-utf16.rs`, verified):

```text
"héllo 😀": 11 UTF-8 bytes, 7 chars, 8 UTF-16 code units
UTF-16 units: [0068, 00E9, 006C, 006C, 006F, 0020, D83D, DE00]
from_utf16(lone surrogate): Err(invalid utf-16: lone surrogate found)
from_utf16_lossy:           "h�i"
from_utf8_lossy([112, 114, 105, 99, 101, 61, 52, 50]) = "price=42" -> Borrowed (no allocation)
from_utf8_lossy([112, 114, 105, 99, 101, 61, 255, 52, 50]) = "price=�42" -> Owned (allocated, U+FFFD inserted)
"Zoë🚀": Rust len() = 8, Java length() would be 5, code points = 4
```

- `encode_utf16()` is a lazy iterator: characters outside the Basic Multilingual Plane become surrogate pairs
  (`😀` → `D83D DE00`).
- `String::from_utf16` must allocate (the UTF-8 result is a different byte sequence), and it fails on a **lone
  surrogate**, which is a legal value in a Java `String` and in a Windows filename but not a Unicode scalar value, so it
  can't exist in a Rust `String`.
- `from_utf8_lossy` returns `Cow<str>`: it borrows when the input is valid (the common case, no allocation) and
  allocates only to insert U+FFFD replacement characters. That's Chapter 3.4's `Cow` pattern applied by std.

**Three length units.** The last line is a classic interop bug in one sentence. Rust's `len()` counts UTF-8 bytes, Java's
`length()` counts UTF-16 code units, and neither counts user-perceived characters (grapheme clusters, which need the
`unicode-segmentation` crate). An API contract that says "max 64 characters" must say which unit, or the Java validator
and the Rust validator will disagree about the same string.

**Case mapping** (listing `ch02-03-case-mapping.rs`, verified):

```text
    straße ( 7 bytes) -> lower "straße" (7 bytes), upper "STRASSE" (7 bytes)
  İstanbul ( 9 bytes) -> lower "i\u{307}stanbul" (10 bytes), upper "İSTANBUL" (9 bytes)
  ΟΔΥΣΣΕΥΣ (16 bytes) -> lower "οδυσσευς" (16 bytes), upper "ΟΔΥΣΣΕΥΣ" (16 bytes)
       ﬁle ( 5 bytes) -> lower "ﬁle" (5 bytes), upper "FILE" (4 bytes)
to_lowercase() == to_lowercase(): 2 allocations
eq_ignore_ascii_case:              0 allocations
make_ascii_lowercase in place:     0 allocations -> "x-request-id"
"STRASSE".eq_ignore_ascii_case("straße") = false
"ÉCOLE".to_ascii_lowercase() = "École"
```

[LIB] `to_lowercase` and `to_uppercase` implement Unicode's default (locale-independent) case mapping:

- **One char can become several.** `ß` uppercases to `SS`, the ligature `ﬁ` to `FI`, and `İ` (capital I with dot)
  lowercases to `i` plus a combining dot above (U+0307). The byte length can grow or shrink.
- **Context matters.** Greek capital sigma lowercases to `ς` at the end of a word and `σ` elsewhere, and `to_lowercase`
  implements that rule (`οδυσσευς`).

Because the output length is unknown and can differ from the input, these functions return a new `String`, and there's
no general in-place version. The ASCII variants (`make_ascii_lowercase`, `eq_ignore_ascii_case`) change only `A`–`Z`,
so they never change the length and never allocate. That makes them right for protocol tokens (HTTP header names,
hex, enum-like identifiers) and wrong for human names (`ÉCOLE` → `École`).

### 5. Memory

**String types by memory** (listing `ch02-04-string-types.rs`, verified):

```text
size_of: String 24, &str 16, Box<str> 16, Arc<str> 16, Cow<str> 24, Option<String> 24
1000 x String::clone:  1001 allocations
1000 x Arc<str>::clone: 1 allocation (the Vec itself); strong_count = 1001
String capacities over 40 pushes: [0, 8, 16, 32, 64]
```

| Type | Handle | Heap | Clone cost | Use it for |
|---|---|---|---|---|
| `String` | 24 bytes | cap bytes | allocate + copy | text you're building or editing |
| `&str` | 16 bytes | borrowed | copy 16 bytes | parameters, parsing views |
| `Box<str>` | 16 bytes | exactly len | allocate + copy | owned text that's frozen: saves 8 bytes per handle plus spare capacity |
| `Arc<str>` | 16 bytes | len + 2 refcounts, one block | atomic increment | text shared by many owners or threads: interned names, config keys |
| `Cow<'a, str>` | 24 bytes | borrowed, or owned | depends | "usually borrowed, sometimes modified" (Chapter 3.4) |

`Cow<str>` and `Option<String>` are both 24 bytes: [RUSTC] the compiler hides their discriminants in a value
`String`'s capacity can never hold (a niche; capacity is at most `isize::MAX`).

**Small-string types** ([LIB], ecosystem, not measured here since they aren't in the Playground's crate set). Crates like
`compact_str` and `smol_str` store short strings inline inside a 24-byte handle, per their documentation up to about 23
or 24 bytes, and allocate only for longer text. `smol_str` is immutable and makes clones O(1) via an `Arc` for long
strings. They pay off when you have millions of short strings (tags, symbols, country codes) and many clones. They cost
a branch on every access, and they add a dependency whose `unsafe` you now rely on. The `SmallVec` measurement in
Chapter 9.1 shows the same trade-off for vectors.

**Millions of short strings: consider not having strings.** If a field has a closed set of values (currency codes,
country codes, statuses), an enum or a `[u8; 3]` is 1–3 bytes with no heap at all. If the set is open but repetitive,
an interner (`HashMap<Box<str>, u32>` plus a `Vec<Box<str>>`) turns each string into a 4-byte ID. Chapter 4.4's
request-scoped interner used this idea.

### 6. CPU / OS

**UTF-8 validation is a scan.** [LIB] `str::from_utf8` has an ASCII fast path that checks a machine word at a time, so
mostly-ASCII input validates at several bytes per cycle; the ecosystem's `simdutf8` uses SIMD to go further. For most
services, validation is not where the time goes. Measure before replacing it with `from_utf8_unchecked`, which is
`unsafe` for a reason: every `str` method assumes valid UTF-8, and a violation is undefined behavior, not a wrong answer.

**Searching bytes: `memchr`, and Project L2's promise.** Project L2 (`redact`) said that if a profile demanded it, the
scanner could "jump between candidate bytes (digits, `@`, `B`, `p`) with `memchr`". Here's what that's worth (listing
`ch02-05-memchr.rs`, release, 16 MiB of log-like text; one noisy run):

```text
haystack: 16 MiB, 315371 lines
rare byte '@' (6308 hits):   iter().position loop    4.64ms | memchr_iter  486.71µs
common byte '\n' (315371 hits): filter().count()       5.65ms | memchr_iter  455.00µs
substring "user=" (6308 hits): str::matches           3.77ms | memmem       441.69µs
class of 13 bytes: table scan 8148655 candidates    3.54ms | memchr3 on '@','B','p' only: 334295 candidates    2.42ms
digits alone are 47% of all bytes
```

[LIB] The `memchr` crate searches for one, two, or three distinct bytes (`memchr`, `memchr2`, `memchr3`) with SIMD,
checking 16 or 32 bytes per step, and `memmem` finds substrings with a SIMD prefilter. It was about 10× faster than the
byte-at-a-time loops here, whether the byte was rare or common, and ~8.5× faster than `str::matches` for a substring.
(`regex` and `aho-corasick` use it internally.)

But the measurement also answers L2's question in the negative. `redact`'s candidate set is **13** bytes, and `memchr`
handles at most three. Worse, **47% of the bytes in this log are digits** (timestamps, IDs, latencies), so "skip to the
next candidate" would stop at nearly every other byte. The right tool for a large byte class is a 256-entry lookup table
(one load and one test per byte, 3.5 ms here), or SIMD byte-class matching as `aho-corasick`'s "Teddy" does internally.
Only the rare triggers (`@`, and a multi-byte prefix like `"Bearer "` via `memmem`) benefit from skipping. That's the
general lesson: a skip-search is only as good as the *rarity* of what it skips to, so measure the candidate density of
your real input before optimizing.

**Windows paths are not UTF-16, and not UTF-8.** [OS] Windows filenames are sequences of 16-bit units that are *usually*
valid UTF-16 but may contain lone surrogates. [LIB] That's why `OsString` exists. On Windows its internal representation
is WTF-8, a UTF-8 superset that can encode lone surrogates, and `std::os::windows::ffi::{OsStrExt::encode_wide,
OsStringExt::from_wide}` convert losslessly to and from `u16` slices. Converting an `OsStr` to `&str` (`to_str`) returns
`None` for such names, and `to_string_lossy` replaces the offending units. A backup tool that uses `to_string_lossy`
for paths can't restore the files it backed up.

**Java interop has two different UTF-8s.** [RUNTIME] This is the edge case that bites JVM teams:

| Java API | Byte format | What Rust sees |
|---|---|---|
| FFM `Arena.allocateFrom(String)` (Java 22+) | standard UTF-8, NUL-terminated | valid UTF-8 (unpaired surrogates were already replaced during encoding) |
| `String.getBytes(StandardCharsets.UTF_8)` | standard UTF-8; malformed input replaced with `?` | valid UTF-8 |
| JNI `GetStringUTFChars` | **Modified UTF-8**: U+0000 as `C0 80`, supplementary characters as two 3-byte surrogates | `str::from_utf8` **rejects** it for any emoji or embedded NUL |
| JNI `GetStringChars` | UTF-16 code units | use `String::from_utf16` |

Meridian's fraud library uses FFM (Chapter 2.2), which gets this right by default. Section 10 is what happened on the
older JNI path.

---

## Pass 3 · Architect level — *Text at scale*

### 7. Trade-offs

**`String` vs `&str`** (SPEC comparison row):

| Criterion | `String` (owned) | `&str` (borrowed) |
|---|---|---|
| Memory | 24-byte handle + capacity | 16-byte handle, no heap of its own |
| CPU | allocate + copy to create | free to create from existing text |
| Latency | allocator calls on the path | none |
| Throughput | allocator-bound in hot loops | bound by the work on the text |
| Contention | allocator contention across threads (Part XX) | none |
| Cache | the text lives wherever the allocator put it | points into the source buffer, often already cached |
| Allocation | ≥ 1 | 0 |
| Complexity | none: owns itself | lifetimes tie it to its source |
| Safety | both fully safe | both fully safe |
| Maintainability | simplest to store in structs | lifetime parameters spread through types |
| Failure modes | allocation storms, retained capacity | "borrowed value does not live long enough" at design time |
| Operational | GC-like memory profile under churn | none |

The architecture rule from Chapter 4.3 still applies: **borrow on the hot path, own at the boundary** where data must
outlive its source (queues, caches, task spawns).

**Choosing an output strategy for serialization:**

1. Format into a **reused `String` or `Vec<u8>`** per worker, and write it to the socket or file in one call.
2. Or format straight into a `BufWriter` (Chapter 3.5): no intermediate string at all, with syscalls amortized.
3. Use `format!` where it's clearest and not hot: error messages, startup logs, tests.

### 8. Java comparison

| Java | Rust | Notes |
|---|---|---|
| `String` (immutable, UTF-16 or Latin-1 "compact strings" since JDK 9, cached hash) | `Box<str>` / `Arc<str>` for immutable, `String` for building | Rust has no single "the string type": ownership and mutability are separate choices. |
| `StringBuilder` (default capacity 16, grows `2n + 2`) | `String` with `push_str` / `write!` | Same model: one growable buffer. |
| `s = s + x` in a loop (quadratic: a new String each time) | `s = s + &x` (reuses the buffer) | **Opposite behavior.** The Rust trap is `format!("{s}...")`. |
| `String.format` (runtime pattern parsing) | `format!` (compile-time parsing) | A bad pattern is a compile error in Rust. |
| `"TITLE".toLowerCase()` (**uses the default locale**) | `to_lowercase()` (locale-independent) | Java's is the famous Turkish-locale bug (`I` → dotless `ı`); Rust's matches Java's `toLowerCase(Locale.ROOT)`. For locale-aware mapping in Rust, use ICU4X. |
| `equalsIgnoreCase` (per-char case folding) | `eq_ignore_ascii_case` (ASCII only) | Unicode caseless matching needs case *folding*, not lowercasing; crates such as `unicase` or `caseless` do it. |
| `length()` = UTF-16 units | `len()` = UTF-8 bytes | Put the unit in every API contract. |
| `intern()` | an interner, or `Arc<str>` | Rust makes you choose where the table lives. |

> **Analogy limit.** Java's `String` is always shared-immutable, so passing it around is free and "who owns this
> text?" never comes up. In Rust it comes up at every struct field. The payoff is that most text in a Rust service is
> never copied at all (borrowed from the request buffer) instead of being copied into a new immutable object at every
> layer, and the cost is that you write `&str` vs `String` decisions into your types.

> **Why not make everything `Arc<str>`?** It's the closest thing to Java's `String`, and for widely shared, rarely
> built text it's the right choice. For text you're building, you'd pay a copy to freeze it, and for text that's
> short-lived, an atomic increment and decrement per clone and drop (Part XIV) buys nothing over a borrow.

### 9. Production scenario

**Access-log formatting in the gateway.** The Java gateway writes one access-log line per request. The first Rust
prototype built it the way the Java code did:
`let line = format!("{} {} {} {} {}", ts, method, path, status, latency_us);` followed by
`logger.write(line)`, plus a `format!` per optional field. The counting-allocator test on recorded traffic showed
four to six allocations per request just for the log line: at 400K requests per second, around two million
allocations per second for logging alone.

The fix:

- Each worker thread owns a `String` with capacity 512.
- The log line is written with `write!` into it, handed to a lock-free queue as a copy into a pre-allocated slot, and
  the buffer is `clear()`ed. A capacity policy caps retained capacity at 4 KiB, per Chapter 9.1.
- Field formatting uses `Display` impls that write directly (`write!(f, "{}", self.0)`) instead of building
  intermediate strings.

The allocations per request for logging went from ~5 to 0. The measured effect on p99 latency belongs to Part XX's
gateway benchmark. Until then it's a hypothesis, with the allocation count as the verified part.

### 10. Failure scenario

**The emoji that changed a fraud score.** Before FFM, Meridian's fraud-feature library had a JNI entry point that took
the customer's display name via `GetStringUTFChars` and hashed it into a feature with `str::from_utf8(bytes)?`.

- **Symptom 1:** about 0.3% of scoring calls returned an error. All were customers with an emoji or other
  supplementary character in their names. JNI's **Modified UTF-8** encodes those as two 3-byte surrogate sequences,
  which standard UTF-8 forbids, so validation failed.
- **The "fix":** an engineer switched to `String::from_utf8_lossy`. The errors disappeared, and every one of those
  names now contained U+FFFD replacement characters. The hashed feature changed for exactly the customers whose names
  had emoji, the model's input distribution shifted for that cohort, and the fraud team spent two weeks investigating
  "model drift" before someone diffed the feature values.
- **Root fix:** take the name through JNI's `GetStringChars` (UTF-16) and decode with `String::from_utf16`, or move the
  entry point to FFM (standard UTF-8). Both preserve the text exactly.
- **Lesson:** `lossy` is a *policy*: "I accept silently changing this data". It's right for log display and wrong for
  anything hashed, compared, stored, or signed. Code review should treat `_lossy` in a data path like an `unwrap` in a
  request path.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part IX).*

1. Why is `s = s + &t` in a loop fine in Rust and quadratic in Java, while `s = format!("{s}{t}")` is quadratic in
   Rust?
2. What does `format!` do at compile time, and what at run time? Why does that design trade some speed for binary size?
3. When does `String::from_utf8_lossy` allocate? When should a code reviewer reject its use?
4. Why can't `String::from_utf16` accept every Java `String`? What does `OsString` do about the Windows equivalent?
5. Why do `to_lowercase` and `to_uppercase` allocate, while `make_ascii_lowercase` works in place? Give three
   characters whose case mapping changes the byte length.
6. What is Modified UTF-8, and where will you meet it?
7. Compare `String`, `Box<str>`, `Arc<str>`, and `Cow<str>` by handle size, heap layout, and clone cost.
8. A field holds one of 180 ISO currency codes, on 50 million records. What type do you give it, and why?
9. When is a small-string crate worth its cost?
10. When does `memchr` speed up a scan, and why doesn't it help a scanner whose candidates are "any digit"?

### 12. Exercises

- **Beginner.** Rewrite `let key = format!("{}:{}", tenant, user_id); cache.get(&key)` so that a lookup allocates
  nothing on the hot path. (Hint: what can a `HashMap<String, _>` be queried with, and could the key be a tuple?)
- **Intermediate.** Extend `ch02-01-string-building.rs` with a seventh variant that writes into a `Vec<u8>` using
  `std::io::Write` and `write!`. Compare allocations and time.
- **Advanced.** Write `fn ascii_lower_cow(s: &str) -> Cow<'_, str>` that borrows when `s` has no ASCII uppercase
  letters and allocates at most once otherwise. Verify with the counting allocator.
- **Systems.** Measure `str::from_utf8` on 100 MB of ASCII versus 100 MB of mixed Greek and CJK text. Explain the
  difference from the implementation's fast path.
- **Architecture.** Your service receives usernames from a Java monolith, a Go service, and a browser. Write the text
  contract: encoding, normalization (NFC?), case rules, and the length unit, and state what each side must validate.

### 13. Debugging exercise

```rust,ignore
fn normalize_header(name: &str) -> String {
    name.to_lowercase()
}

fn is_hop_by_hop(name: &str) -> bool {
    let n = normalize_header(name);
    n == "connection" || n == "keep-alive" || n == "transfer-encoding" || n == "upgrade"
}
```

1. A profile of the gateway shows `to_lowercase` in the top 20 functions. How many allocations does `is_hop_by_hop`
   make per call, and how many calls are there per request?
2. There's also a correctness problem: find a header name for which `to_lowercase` produces a match that an HTTP/1.1
   parser should reject. (Hint: header names are ASCII tokens by specification; what does `to_lowercase` do with the
   Kelvin sign, U+212A?)
3. Rewrite both functions with zero allocations and ASCII-only semantics.

### 14. Design exercise

**The ledger's statement export.** Meridian's ledger (Java, ~3K TPS) is getting a Rust service that renders monthly
statements: about 2 million PDFs and CSVs overnight, each with 50 to 5,000 lines of descriptions, amounts, and
dates, some in Greek, Turkish, and Japanese.

Design the text pipeline: the encoding contract with the Java ledger (FFM, JSON over HTTP, or Protobuf), where text is
borrowed and where it's owned, how each line is formatted (and with what buffer strategy), how amounts are rendered
(integer minor units, Chapter 2.3), and how case-insensitive merchant-name matching works across scripts. State the
allocation budget per statement and how you'll test that the Turkish customers' statements are correct.
