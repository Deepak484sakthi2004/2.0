# Chapter 3.4 — Slices, String, and &str

> **Where this sits:** Part III · Ownership · chapter 4 of 6
> **Prerequisites:** Chapters 3.1–3.3.
> **After this chapter you can:** choose between `String`, `&str`, `Box<str>`, `Arc<str>`, and `Cow<str>` (and their
> `[T]` counterparts) deliberately; explain fat pointers and dynamically sized types; handle UTF-8 correctly (bytes vs
> chars vs what users see); and avoid the production panic that byte-index slicing causes on non-ASCII text.

---

## Pass 1 · User level — *Owned text and borrowed views*

### 1. Problem

Backend systems move text and bytes around more than anything else: headers, JSON, log lines, IDs, names. Java hides
most of the choices. Every `String` is an immutable heap object, freely shared, indexed by UTF-16 code units. Rust makes
two things explicit:

1. **Ownership of text.** Do you own this string (and may grow or mutate it), or are you looking at someone else's?
2. **Encoding.** Rust strings are **always valid UTF-8**, so "the 10th character" isn't an O(1) index, and a byte offset
   can land in the middle of a character.

Get both right and text handling is zero-copy and correct. Get either wrong and you either allocate everywhere or ship
a panic that only non-English names trigger.

### 2. Mental model

**Owned/borrowed pairs.** Rust has the same pattern for every kind of contiguous data:

| Owned (can grow, frees on drop) | Borrowed view | Contents |
|---|---|---|
| `String` | `&str` | UTF-8 text |
| `Vec<T>` | `&[T]` / `&mut [T]` | Any `T`, contiguous |
| `PathBuf` | `&Path` | Filesystem paths (OS-encoded) |
| `OsString` | `&OsStr` | OS strings, possibly not UTF-8 |
| `Box<str>` / `Box<[T]>` | — | Owned but **fixed-size**: no capacity, can't grow |
| `Arc<str>` / `Arc<[T]>` | — | Shared, immutable, cheap to clone (a refcount bump) |
| `Cow<'a, str>` / `Cow<'a, [T]>` | — | Borrowed *or* owned, decided at run time |

`str` and `[T]` are **dynamically sized types** (DSTs). Their length isn't known at compile time, so you never hold one
directly. You always hold it behind a pointer that carries the length: `&str`, `Box<str>`, `Arc<str>`. Those pointers are
**fat**: address plus length, 16 bytes on 64-bit.

```text
 String (24 bytes, owns)             &str (16 bytes, borrows)          the bytes (heap, UTF-8)
 ┌───────────┐                       ┌───────────┐                     ┌──┬─────┬──┬──┬──┬──┬──┬──┬─────┬──┬──┬──┐
 │ ptr ──────┼──────────────────────►│           │                     │h │ é   │l │l │o │  │w │ ö   │r │l │d │
 │ cap = 13  │                       │ ptr ──────┼────────────────────►│68│c3 a9│6c│6c│6f│20│77│c3 b6│72│6c│64│
 │ len = 13  │                       │ len = 6   │  (a view of "héllo") └──┴─────┴──┴──┴──┴──┴──┴─────┴──┴──┴──┘
 └───────────┘                       └───────────┘                      0  1  2  3  4  5  6  7  8  9 10 11 12
```

**Bytes, chars, and what a person sees are three different counts:**

| Unit | Rust API | "héllo wörld" | Note |
|---|---|---|---|
| Bytes | `s.len()`, `s.as_bytes()` | 13 | O(1). What slicing indices mean. |
| Unicode scalar values | `s.chars()` | 11 | O(n) to count or index. `char` is 4 bytes. |
| Grapheme clusters (user-perceived characters) | not in std (`unicode-segmentation` crate) | 11 here | "é" may be one scalar *or* "e" + a combining accent (two) |

### 3. Rust code

The essentials, verified:

```rust
use std::mem::size_of;

fn shout(text: &str) -> String {
    // accepts &str: works for String, literals, and slices alike
    text.to_uppercase()
}

fn main() {
    println!(
        "sizes: String={} &str={} Box<str>={} Vec<u8>={} &[u8]={}",
        size_of::<String>(), size_of::<&str>(), size_of::<Box<str>>(), size_of::<Vec<u8>>(), size_of::<&[u8]>()
    );

    let s = String::from("héllo wörld");
    println!("len() = {} bytes, chars().count() = {}", s.len(), s.chars().count());
    for (i, ch) in s.char_indices().take(3) {
        println!("  byte {i}: {ch:?} ({} byte(s) in UTF-8)", ch.len_utf8());
    }
    println!("'é' is encoded as {:x?}", "é".as_bytes());

    let hello: &str = &s[0..6]; // "héllo": h(1) é(2) l(1) l(1) o(1) = 6 bytes
    println!("&s[0..6] = {hello:?}; is_char_boundary(2) = {}", s.is_char_boundary(2));

    let owned: String = hello.to_owned(); // an independent copy on the heap
    println!("owned = {owned:?}, capacity {}", owned.capacity());

    println!("{}", shout(&s)); // &String coerces to &str (deref coercion)
    println!("{}", shout("literal")); // a &'static str baked into the binary
    println!("{}", shout(&s[7..])); // a sub-slice: no copy
}
```

```text
sizes: String=24 &str=16 Box<str>=16 Vec<u8>=24 &[u8]=16
len() = 13 bytes, chars().count() = 11
  byte 0: 'h' (1 byte(s) in UTF-8)
  byte 1: 'é' (2 byte(s) in UTF-8)
  byte 3: 'l' (1 byte(s) in UTF-8)
'é' is encoded as [c3, a9]
&s[0..6] = "héllo"; is_char_boundary(2) = false
owned = "héllo", capacity 6
HÉLLO WÖRLD
LITERAL
WÖRLD
```

Byte 2 is the second byte of `é` (`a9`), not a boundary. `&s[7..]` starts at `w` (after the space at byte 6), which is a
boundary, so the slice is valid. And indexing a single "character" by position doesn't compile at all:

```rust,compile_fail
fn main() {
    let s = String::from("hello");
    let c = s[0];
    println!("{c}");
}
```

```text
error[E0277]: the type `str` cannot be indexed by `{integer}`
 --> src/main.rs:4:15
  |
4 |     let c = s[0];
  |               ^ string indices are ranges of `usize`
  |
  = help: the trait `SliceIndex<str>` is not implemented for `{integer}`
  = note: you can use `.chars().nth()` or `.bytes().nth()`
```

That refusal is deliberate. Should `s[0]` be a byte (not a character in general), a `char` (O(n) to find), or a
grapheme (not in std)? Any answer would be either wrong for non-ASCII text or silently O(n). Rust makes you say which:
`s.as_bytes()[0]`, `s.chars().nth(0)`, or a range `&s[0..1]` that panics if it isn't on a boundary.

---

## Pass 2 · Systems level — *The UTF-8 invariant and fat pointers*

### 4. Under the hood

**`String` is a `Vec<u8>` with an invariant.** [LANG] A `str` must always contain valid UTF-8. Producing a `str` that
doesn't is undefined behavior, and every safe API upholds the rule:

- `String::from_utf8(bytes)` **validates** (O(n)) and returns `Err` on bad input. `String::from_utf8_lossy` replaces bad
  sequences with U+FFFD and returns a `Cow` (borrowed when no replacement was needed; Project Level 2 relies on this).
- `str::from_utf8_unchecked` skips validation. It's `unsafe`, and the caller promises validity (Part XV).
- **Slicing checks char boundaries** and panics otherwise (§10). Because the check happens at run time, a byte-index
  slice is fine for ASCII input and a crash for other input, so it's a classic "works in testing" bug.

**Deref coercion.** `&String` automatically becomes `&str` wherever a `&str` is expected, because `String: Deref<Target
= str>` (Part VI explains `Deref`). The same holds for `&Vec<T>` → `&[T]` and `&Box<T>` → `&T`. That's why `&str` and
`&[T]` parameters accept everything, and why `&String` parameters are an anti-pattern: they accept *less* and give the
function nothing extra.

**String literals** are `&'static str`: pointers into the binary's read-only data section, valid for the entire run.
Nothing is allocated. `"literal".to_string()` allocates a copy.

**Slices in general.** `&v[a..b]` is O(1): new pointer = old pointer + `a × size_of::<T>()`, new length = `b − a`, plus
bounds checks. `split_at`, `chunks`, `windows`, and `split_first` all return views without copying. For `[u8]` and `str`
this is how zero-copy parsers work: Chapter 2.4's header parser, `logstat`'s `Record<'a>`, and the project in this Part.

### 5. Memory

**UTF-8 encodes a character in 1 to 4 bytes**, with self-describing leading bits:

```text
 U+0000 – U+007F      0xxxxxxx                                ASCII: 1 byte (h, l, space)
 U+0080 – U+07FF      110xxxxx 10xxxxxx                       é = c3 a9, ö = c3 b6, Ø = c3 98
 U+0800 – U+FFFF      1110xxxx 10xxxxxx 10xxxxxx              李 = e6 9d 8e, ✓ = e2 9c 93
 U+10000 – U+10FFFF   11110xxx 10xxxxxx 10xxxxxx 10xxxxxx     emoji: 😀 = f0 9f 98 80

 continuation bytes are ALWAYS 10xxxxxx (0x80–0xBF): that's what is_char_boundary checks
```

**Growth and capacity** work as for `Vec` (Part IX): `push_str` may reallocate, and `String::with_capacity(n)` avoids
that when you know the size. [LIB] `to_owned()` on a `&str` allocates exactly `len` bytes (the `capacity 6` above).

**No small-string optimization in std.** [LIB] Every non-empty `String` owns a heap buffer, even for "ok". (`String::new()`
doesn't allocate.) C++'s libstdc++ stores strings of up to 15 bytes inline. Rust's std doesn't, which keeps `String` a
simple, predictable `Vec<u8>`. Crates such as `compact_str` and `smol_str` add inline storage when many short strings
dominate memory.

**`Box<str>` vs `String`:** 16 bytes vs 24, and no spare capacity. For millions of immutable strings, such as keys in a
large map, `Box<str>` (via `into_boxed_str()`, which shrinks the buffer to fit) saves 8 bytes per string plus any unused
capacity. **`Arc<str>`** shares one immutable buffer among many owners: cloning is a refcount increment, not a copy.

### 6. CPU / OS

- **UTF-8 is self-synchronizing.** No ASCII byte ever appears inside a multi-byte character, and continuation bytes are
  distinguishable from leading bytes. So byte-level search for ASCII delimiters (`\n`, `,`, `@`, a space) is *correct*
  on UTF-8 text, and fast (`memchr` uses SIMD to scan many bytes per instruction). Project Level 2 uses this property
  to scan bytes and still slice safely.
- **Validation isn't free, but it's fast.** Checking UTF-8 is O(n) and, with SIMD, runs at multiple GB/s on modern
  cores. Validate once at the boundary (reading a request, a file), then work with `&str` knowing it's valid.
- **ASCII fast paths.** `to_ascii_lowercase`, `eq_ignore_ascii_case`, and byte comparisons avoid Unicode tables.
  `to_lowercase`/`to_uppercase` implement full Unicode case mapping. They're correct for human text, slower, and they
  can change a string's length ("ß" uppercases to "SS").
- **OS strings aren't necessarily UTF-8.** [OS] A Linux filename is arbitrary bytes. Windows file names are UTF-16 that
  may contain unpaired surrogates. `OsStr` and `Path` represent those faithfully, and `to_str()` returns `Option<&str>`
  because the conversion can fail. Code that handles file paths as `String` breaks on real filesystems.

---

## Pass 3 · Architect level — *Choosing text representations*

### 7. Trade-offs

| Type | Size | Owns? | Mutable? | Clone cost | Use for |
|---|---|---|---|---|---|
| `&str` | 16 | No | No | A pointer copy | Parameters; views into buffers; zero-copy parsing |
| `String` | 24 | Yes | Yes (grow, push) | Allocation + copy | Building text; owned fields that change |
| `Box<str>` | 16 | Yes | No | Allocation + copy | Many owned immutable strings (map keys) |
| `Arc<str>` | 16 | Shared | No | A refcount increment | Immutable strings shared across structures or threads |
| `Cow<'a, str>` | 24 | Borrowed *or* owned | On demand | Borrowed: none | "Usually unchanged, sometimes modified" |
| An interned ID (`u32` into a table) | 4 | The table does | No | Free | Huge numbers of repeated values (tags, paths, tenants) |

**`Cow`: borrow when you can, own when you must.** A function that *sometimes* has to change its input returns
`Cow<'_, str>`, borrowed when nothing changed and owned when something did. The caller treats both the same (a `Cow`
derefs to `&str`), and only pays for allocation when modification actually happened. §9 measures it.

### 8. Java comparison

| Java | Rust | Note |
|---|---|---|
| `String`: immutable, shared freely | `String`: owned, mutable; `&str` / `Arc<str>` to share | Java can share because strings are immutable. Rust shares through borrowing or `Arc`. |
| UTF-16 internally (Latin-1 "compact strings" since Java 9) | UTF-8, always | Rust's `len()` is bytes. Java's `length()` is UTF-16 code units. Neither is "characters." |
| `charAt(i)`: O(1), returns a UTF-16 unit (maybe half a surrogate pair) | No `s[i]`; `chars().nth(i)` is O(n) and returns a full scalar value | Rust makes the O(n) visible |
| `substring` | `&s[a..b]` (a borrow, O(1)) or `.to_owned()` (a copy) | See below |
| `StringBuilder` | `String::with_capacity` + `push_str` / `write!` | — |
| `==` is reference equality; `.equals` is content | `==` compares content | — |

**The `substring` story is an ownership story.** Until Java 7u6, `substring` returned a view sharing the original's
`char[]`. That was fast, but a 10-character substring of a 10 MB string kept the whole 10 MB alive, a notorious memory
leak. Java 7u6 changed `substring` to *copy*, trading speed for predictability. Rust keeps both options, **explicitly**.
`&s[a..b]` is the fast view, and its lifetime keeps the original alive, *visibly*, in the type. `.to_owned()` is the
copy. The compiler shows you exactly when you're holding a view into something big.

> **Analogy limit.** "`&str` is Java's `String`" is wrong in the way that matters. A Java `String` is an owned,
> independently living object. A `&str` is a *loan* on someone else's bytes, and it can't outlive them. The closest
> Java analog to `&str` is a `CharSequence` view, and the closest to `String` is a `StringBuilder` you own.

### 9. Production scenario

**Meridian's header normalization with `Cow`.** HTTP header names are case-insensitive, and the gateway normalizes them
to lowercase. In real traffic almost all names already are, so a `String` per header would mean an allocation per header
per request for no reason (listing `ch04-05-cow.rs`, verified):

```rust,ignore
use std::borrow::Cow;

/// HTTP header names are case-insensitive; normalize to lowercase. Most already are.
fn normalize_header(name: &str) -> Cow<'_, str> {
    if name.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(name.to_ascii_lowercase()) // pay for an allocation only when something changes
    } else {
        Cow::Borrowed(name) // zero-copy
    }
}
```

```text
    content-type -> content-type     (borrowed)
    x-request-id -> x-request-id     (borrowed)
          Accept -> accept           (owned)
   authorization -> authorization    (borrowed)
 X-Forwarded-For -> x-forwarded-for  (owned)
      user-agent -> user-agent       (borrowed)
allocations: 3 (one for the Vec, one per header that had to change)
```

Measured by the counting allocator: normalizing six headers made exactly two string allocations, one for each header
that actually changed. At 400K requests per second with about 15 headers each, the difference between "always allocate"
and "allocate only when changed" is millions of allocations per second. The design costs one type in the signature,
and it states the contract honestly: *this may or may not have copied.*

### 10. Failure scenario

**The name that crashed the profile page.** A Meridian service truncates display names to fit a UI column:

```rust
fn display_name(full: &str) -> &str {
    // BUG: 10 is a BYTE index, not a character count
    if full.len() > 10 { &full[..10] } else { full }
}

fn main() {
    println!("{}", display_name("Ada Lovelace"));
    println!("{}", display_name("Kristina Øberg"));
}
```

```text
Ada Lovela
thread 'main' (14) panicked at src/main.rs:4:31:
end byte index 10 is not a char boundary; it is inside 'Ø' (bytes 9..11 of string)
```

It passed every test (all test names were ASCII) and panicked for the first customer whose 10th byte fell inside a
two-byte character. The panic killed the request, and because the name was on the profile page, it did so on *every*
request for that customer. The fix slices at a boundary found by counting characters (verified):

```rust,ignore
/// The first `max_chars` characters of `full`, as a borrowed slice (no allocation).
fn display_name(full: &str, max_chars: usize) -> &str {
    match full.char_indices().nth(max_chars) {
        Some((byte_index, _)) => &full[..byte_index], // always a char boundary
        None => full,
    }
}
```

```text
Ada Lovelace         -> "Ada Lovela"
Kristina Øberg       -> "Kristina Ø"
Zoë                  -> "Zoë"
李小龍 Bruce Lee        -> "李小龍 Bruce "
```

Look at the last line's alignment. `{:<20}` pads to 20 **chars**, but CJK characters are typically displayed **two
columns wide**, so the column is visibly off. Chars aren't display width either. For truly user-facing truncation, count
**grapheme clusters** (`unicode-segmentation`) and, for alignment, **display width** (`unicode-width`). The architectural
rule is to decide *which unit* each layer means. Storage limits are usually bytes, API limits are usually chars, and UI
limits are graphemes or display width. Then write it in the types or the names (`max_bytes`, `max_chars`).

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part III).*

1. Why does `String` take 24 bytes and `&str` 16? What is a fat pointer, and what is a dynamically sized type?
2. Why can't you index a `String` with `s[0]`? What are the three things "the first character" could mean?
3. What does `String`'s UTF-8 invariant guarantee, where is it checked, and what happens if `unsafe` code violates it?
4. Why is a `&String` parameter an anti-pattern? What does deref coercion have to do with it?
5. Why is byte-level searching for ASCII delimiters correct on UTF-8 text?
6. When would you store `Box<str>` instead of `String`? `Arc<str>` instead of either?
7. Explain `Cow<'a, str>`. When does it pay off, and what does it cost when it doesn't?
8. Tell the Java `substring` memory-leak story and explain how Rust handles the same trade-off.

### 12. Exercises

- **Beginner.** Write `fn initials(full_name: &str) -> String` that works for "Ada Lovelace", "Zoë Saldaña", and
  "李 小龍". What does "initial" even mean for the last one?
- **Intermediate.** Write `fn escape_html(s: &str) -> Cow<'_, str>` that escapes `<`, `>`, `&`, and `"` and returns
  `Borrowed` when there's nothing to escape. Measure allocations for clean and dirty inputs.
- **Advanced.** Implement `fn split_fields(line: &str) -> Vec<&str>` for a CSV subset with quoted fields (quotes can
  contain commas). Keep it zero-copy for unquoted fields. What do you do with escaped quotes (`""`) inside a quoted
  field, where the output can't be a slice of the input?
- **Systems.** Compare the throughput of `s.to_lowercase()` and `s.to_ascii_lowercase()` on a 100 MB ASCII string and
  on a 100 MB string of mixed scripts. Explain the difference. (Measure it. Don't guess the ratio.)
- **Architecture.** For a system you know, list every place text crosses a boundary (HTTP, database, message queue,
  logs, UI). For each, state the encoding, the length unit, the maximum length, and who validates.

### 13. Debugging exercise

`let c = s[0];` fails with E0277: *the type `str` cannot be indexed by `{integer}`*.

1. The note suggests `.chars().nth()` or `.bytes().nth()`. When is each correct? Give an input where they return
   different "first characters."
2. A colleague writes `&s[0..1]` instead. When does it compile, when does it panic, and why is that worse than a
   compile error for some inputs?
3. Write a function `first_char(s: &str) -> Option<char>` that's O(1) and correct for all inputs. Why is it O(1) even
   though `chars().nth(k)` is O(k)?

### 14. Design exercise

**Customer names across Meridian's layers.** Names enter through the signup API (JSON over HTTP), are stored in
PostgreSQL (`VARCHAR(100)`), appear in the UI (truncated to fit), in CSV exports, in logs (after redaction), and in
search (normalized for matching).

For each layer, decide: the Rust type holding the name (`String`, `&str`, `Box<str>`, `Arc<str>`, `Cow<str>`, an
interned ID), the length unit and limit, the normalization (Unicode NFC? case folding?), and where validation happens.
Pay special attention to `VARCHAR(100)`: in PostgreSQL that's 100 *characters* (a UTF-8 database stores more bytes).
What goes wrong if one layer counts bytes and another counts characters? Write the rule you'd put in the team's API
guidelines.
