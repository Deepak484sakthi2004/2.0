# Chapter 8.1 — Result, Option, and the `?` Operator

> **Where this sits:** Part VIII · Error Handling · chapter 1 of 4
> **Prerequisites:** Chapters 2.5–2.6 (enums, `match`, niches), 3.5 (`Drop` on early return), Project L1 (`logstat`).
> **After this chapter you can:** choose between `Option` and `Result` for a given failure; write fallible code with
> `?` and read exactly what it expands to (verified in MIR); convert between `Option` and `Result` and between error
> types; read the two E0277 errors that `?` produces; predict the size of a `Result` from niches; and show in real
> assembly that the happy path of `?` costs one test and one branch.

---

## Pass 1 · User level — *Failure as a value*

### 1. Problem

Every non-trivial function can fail, and the caller has to find out. Languages have tried three designs:

| Design | Examples | The failure path is... |
|---|---|---|
| **Exceptions** | Java, C#, Python, C++ | invisible in the call expression; declared in the signature only for Java's checked exceptions |
| **Error codes / extra return value** | C (`errno`, `-1`), Go (`val, err`) | visible, but ignorable: nothing forces you to check it |
| **Sum types** | ML, Haskell, Rust | part of the return type: you can't get the value without deciding what to do about the failure |

Meridian's gateway reads `Integer.parseInt(System.getenv("POOL_SIZE"))` at startup. The signature of `parseInt` says it
returns an `int`. It also throws `NumberFormatException`, an unchecked exception that appears nowhere in the call. In
Rust, `"32".parse::<u32>()` returns `Result<u32, ParseIntError>`. You can't do arithmetic on it until you've said what
happens in the `Err` case.

That creates a new problem: if every fallible call needs a `match`, the success path drowns in boilerplate. Go shows
what that looks like (`if err != nil { return err }` after every call). Rust's answer is the **`?` operator**: one
character that says "if this failed, return the failure to my caller, converted to my error type; otherwise give me the
value." This chapter covers the two types, the operator, and what both cost at run time.

### 2. Mental model

Two enums from the standard library [LANG]:

```rust,ignore
enum Option<T> { None, Some(T) }      // absence: a normal outcome, no reason attached
enum Result<T, E> { Ok(T), Err(E) }   // failure: an outcome with a reason
```

**The rule:** use `Option` when "nothing" is a legitimate answer the caller expects, and `Result` when something went
wrong and the caller may need to know *what*.

| Operation | Returns | Why |
|---|---|---|
| `HashMap::get(&k)` | `Option<&V>` | A missing key is a normal answer |
| `Vec::pop()`, `Iterator::next()` | `Option<T>` | Empty is expected; it's how loops end |
| `str::parse::<u32>()` | `Result<u32, ParseIntError>` | The caller may want to report *why* ("invalid digit" vs "too large") |
| `File::open(path)` | `io::Result<File>` | Not found, permission denied, and too many open files need different handling |
| `slice.first()` | `Option<&T>` | Empty slice is normal |
| `u32::checked_add(x)` | `Option<u32>` | Overflow has one possible reason, so there's nothing to report |

**`?` in one picture.** Written after an expression of type `Result` or `Option`, inside a function that returns a
compatible type:

```text
  expr?
    ├── Ok(v)  / Some(v)  ──►  evaluates to v, and execution continues
    └── Err(e) / None     ──►  return Err(From::from(e))  /  return None      (early return from the ENCLOSING fn)
```

Three properties matter:

1. **It is a return.** `?` exits the enclosing function, so every `?` is an exit path. Chapter 3.5's guarantee is what
   makes that safe: locals are dropped on every exit, so there is no `finally` to write.
2. **It converts the error.** The `Err` value passes through `From::from`, so a function returning `Result<_, ConfigError>`
   can `?` a `ParseIntError` if (and only if) `impl From<ParseIntError> for ConfigError` exists.
3. **It is visible.** Unlike an exception, you can see every point where control may leave the function, by scanning
   for `?`.

**Moving between the two types:**

```text
 Option<T> ──.ok_or(err) / .ok_or_else(|| err)──► Result<T, E>       "absence is an error HERE"
 Result<T,E> ──.ok()──► Option<T>                                    "I don't care why it failed" (discards E!)
 Option<Result<T,E>> ◄──.transpose()──► Result<Option<T>,E>          "maybe absent, and fallible if present"
```

### 3. Rust code

A configuration lookup that uses both types for what each is for (listing `ch01-01-option-result.rs`, verified):

```rust
use std::collections::HashMap;

/// Absence is not an error: Option.
fn find_timeout<'a>(config: &HashMap<&str, &'a str>) -> Option<&'a str> {
    config.get("timeout_ms").copied()
}

/// Failure with a reason: Result.
fn parse_timeout(raw: &str) -> Result<u64, std::num::ParseIntError> {
    raw.trim().parse::<u64>()
}

/// Combining them: missing -> a default; present but malformed -> an error.
fn timeout_ms(config: &HashMap<&str, &str>) -> Result<u64, String> {
    match find_timeout(config) {
        None => Ok(1_000),
        Some(raw) => parse_timeout(raw).map_err(|e| format!("timeout_ms={raw:?}: {e}")),
    }
}

fn main() {
    let ok = HashMap::from([("timeout_ms", " 250 ")]);
    let missing: HashMap<&str, &str> = HashMap::new();
    let bad = HashMap::from([("timeout_ms", "25O")]); // a letter O, not a zero
    for (name, cfg) in [("ok", &ok), ("missing", &missing), ("bad", &bad)] {
        println!("{name:>7}: {:?}", timeout_ms(cfg));
    }

    // Combinators express the same kind of logic without an explicit match:
    let retries: Option<u32> = Some("3").and_then(|s| s.parse().ok()).filter(|&n| n <= 10);
    let port: Result<u16, String> = "70000".parse::<u16>().map_err(|e| e.to_string());
    println!("retries={retries:?} port={port:?}");
    println!("ok_or: {:?}", None::<u32>.ok_or("missing port"));
    println!("unwrap_or: {}", "x".parse::<u32>().unwrap_or(8080));
}
```

```text
     ok: Ok(250)
missing: Ok(1000)
    bad: Err("timeout_ms=\"25O\": invalid digit found in string")
retries=Some(3) port=Err("number too large to fit in target type")
ok_or: Err("missing port")
unwrap_or: 8080
```

The `String` error type is a placeholder: Chapter 8.2 replaces it. The important design decision is already visible: a
**missing** key falls back to a default, and a **malformed** one is an error. §9 shows what happens at Meridian
when those two get merged.

**`?` by hand, then with `?`.** The same function written twice (listing `ch01-02-question-mark.rs`, verified; the
`Display` and `Error` impls are elided here and shown in full in the listing):

```rust,ignore
#[derive(Debug)]
enum ConfigError {
    Missing(&'static str),
    Invalid(ParseIntError),
}

// This impl is what lets `?` turn a ParseIntError into a ConfigError.
impl From<ParseIntError> for ConfigError {
    fn from(e: ParseIntError) -> Self {
        ConfigError::Invalid(e)
    }
}

fn get<'a>(cfg: &HashMap<&str, &'a str>, key: &'static str) -> Result<&'a str, ConfigError> {
    cfg.get(key).copied().ok_or(ConfigError::Missing(key))
}

/// What `?` does, written out by hand.
fn parse_port_by_hand(cfg: &HashMap<&str, &str>) -> Result<u16, ConfigError> {
    let raw = match get(cfg, "port") {
        Ok(v) => v,
        Err(e) => return Err(From::from(e)),
    };
    let port = match raw.parse::<u16>() {
        Ok(v) => v,
        Err(e) => return Err(From::from(e)), // uses the From<ParseIntError> impl
    };
    Ok(port)
}

/// The same function with `?`.
fn parse_port(cfg: &HashMap<&str, &str>) -> Result<u16, ConfigError> {
    let port = get(cfg, "port")?.parse::<u16>()?;
    Ok(port)
}

/// `?` also works on Option, inside a function that returns Option.
fn first_even_square(xs: &[u32]) -> Option<u32> {
    let first = xs.iter().find(|x| *x % 2 == 0)?; // None -> return None
    first.checked_mul(*first) // None on overflow
}
```

For `{"port": "8080"}`, an empty map, and `{"port": "80x"}`, both versions agree:

```text
Ok(8080)                           same=true cause=None
Err(Missing("port"))               same=true cause=None
Err(Invalid(ParseIntError { kind: InvalidDigit })) same=true cause=Some("invalid digit found in string")
Some(16) None None
```

(The last line: `[3, 4, 5]` → 16; `[1, 3]` has no even element; `70_000²` overflows `u32`.) Note the first `?` in
`parse_port`: `get` already returns `ConfigError`, so `From::from` is the identity conversion `impl<T> From<T> for T`.

**The two errors `?` gives you.** Using `?` in a function that returns `()` (listing `ch01-03-question-in-main.rs`):

```rust,compile_fail
fn main() {
    let port: u16 = "8080".parse()?;
    println!("{port}");
}
```

```text
error[E0277]: the `?` operator can only be used in a function that returns `Result` or `Option` (or another type that implements `FromResidual`)
 --> src/main.rs:3:35
  |
2 | fn main() {
  | --------- this function should return `Result` or `Option` to accept `?`
3 |     let port: u16 = "8080".parse()?;
  |                                   ^ cannot use the `?` operator in a function that returns `()`
```

And using `?` when no `From` conversion exists (listing `ch01-04-missing-from.rs`):

```rust,compile_fail
#[derive(Debug)]
enum ConfigError {
    Missing(&'static str),
}

fn parse_port(raw: Option<&str>) -> Result<u16, ConfigError> {
    let raw = raw.ok_or(ConfigError::Missing("port"))?;
    let port = raw.parse::<u16>()?; // no From<ParseIntError> for ConfigError
    Ok(port)
}
```

```text
error[E0277]: `?` couldn't convert the error to `ConfigError`
 --> src/main.rs:9:34
  |
7 | fn parse_port(raw: Option<&str>) -> Result<u16, ConfigError> {
  |                                     ------------------------ expected `ConfigError` because of this
8 |     let raw = raw.ok_or(ConfigError::Missing("port"))?;
9 |     let port = raw.parse::<u16>()?; // no From<ParseIntError> for ConfigError
  |                    --------------^ the trait `From<ParseIntError>` is not implemented for `ConfigError`
  = note: the question mark operation (`?`) implicitly performs a conversion on the error value using the `From` trait
```

Both are E0277 ("trait not implemented") because both are trait questions. The first asks whether `()` implements
`FromResidual`. The second asks whether `ConfigError` implements `From<ParseIntError>`. §4 shows where those traits
come from.

**`main` can return `Result`** (listing `ch01-05-main-result.rs`):

```rust,no_run
use std::num::ParseIntError;

// main may return Result: an Err is printed with Debug to stderr and the exit status is 1.
fn main() -> Result<(), ParseIntError> {
    let port: u16 = "8080".parse()?;
    println!("port = {port}");
    let workers: u16 = "eight".parse()?;
    println!("workers = {workers}");
    Ok(())
}
```

```text
stdout: port = 8080
stderr: Error: ParseIntError { kind: InvalidDigit }
```

Note that it prints **`Debug`**, not `Display`. That's fine for a quick tool and wrong for a product. Chapter 8.2 shows
how `anyhow` makes its `Debug` output readable, and why `logstat` maps errors to exit codes itself. (Chapter 8.3 verifies
the exit status, 1, by re-running a binary as a child process.)

**`Result` is `#[must_use]`.** Dropping one on the floor compiles, with a warning (listing `ch01-08-must-use.rs`):

```rust
use std::fs;

fn main() {
    // Release the batch job's lock. If this fails, the next run will refuse to start.
    fs::remove_file("/tmp/meridian-settlement.lock");
    println!("lock released (or was it?)");
}
```

```text
warning: unused `Result` that must be used
 --> src/main.rs:6:5
  |
6 |     fs::remove_file("/tmp/meridian-settlement.lock");
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  |
  = note: this `Result` may be an `Err` variant, which should be handled
  = note: `#[warn(unused_must_use)]` (part of `#[warn(unused)]`) on by default
help: use `let _ = ...` to ignore the resulting value
```

§10 is what happened when that warning scrolled past unread.

---

## Pass 2 · Systems level — *What `?` compiles to*

### 4. Under the hood

**The desugaring.** [LANG] The Reference defines `?` for `Result` and `Option`. Internally, rustc lowers every `?` to
calls on two traits, `Try` and `FromResidual`, which are **unstable** [VERSION] (feature `try_trait_v2`): you can't
implement them for your own types on stable, but std's types use them. Conceptually:

```rust,ignore
// expr? becomes (conceptual; the traits are unstable, and the real lowering happens in HIR):
match Try::branch(expr) {
    ControlFlow::Continue(v) => v,
    ControlFlow::Break(residual) => return FromResidual::from_residual(residual),
}
```

`branch` splits a value into "keep going with `v`" or "stop with a **residual**", the part that carries the failure.
For `Result<T, E>`, the residual is `Result<Infallible, E>`: an `Err` that provably can't be `Ok`. For `Option<T>`, it's
`Option<Infallible>`, meaning `None`. `from_residual` rebuilds the *function's* return type from the residual, and for
`Result` [LIB] that's where the conversion happens:

```rust,ignore
// std (simplified): the one impl that makes `?` convert errors.
impl<T, E, F: From<E>> FromResidual<Result<Infallible, E>> for Result<T, F> {
    fn from_residual(residual: Result<Infallible, E>) -> Self {
        match residual {
            Err(e) => Err(From::from(e)),
        }
    }
}
```

This explains both E0277s from §3. `()` has no `FromResidual` impl, and `F: From<E>` is the bound that failed for
`ConfigError`. It also explains why `?` on an `Option` inside a function returning `Result` fails (the debugging
exercise): there's no impl of `FromResidual<Option<Infallible>>` for `Result`, by design, because a `None` carries no
error to convert.

**The real MIR.** Here's the verified MIR (debug build, rustc 1.98.1) of a function with two `?`s (listing
`ch01-07-happy-path.rs`):

```rust,ignore
#[inline(never)]
pub fn sum_two(a: &str, b: &str) -> Result<u32, ParseIntError> {
    let x = parse_u32(a)?;
    let y = parse_u32(b)?;
    Ok(x.wrapping_add(y))
}
```

```text
bb0: { _4 = parse_u32(copy _1) -> [return: bb1, unwind continue]; }
bb1: { _3 = <Result<u32, ParseIntError> as Try>::branch(move _4) -> [return: bb2, unwind continue]; }
bb2: { _5 = discriminant(_3);
       switchInt(move _5) -> [0: bb4, 1: bb5, otherwise: bb3]; }
bb4: { _7 = copy ((_3 as Continue).0: u32);                       // x = v
       _9 = parse_u32(copy _2) -> [return: bb6, unwind continue]; }
bb5: { _6 = move ((_3 as Break).0: Result<Infallible, ParseIntError>);
       _0 = <Result<u32, ParseIntError> as FromResidual<Result<Infallible, ParseIntError>>>
              ::from_residual(copy _6) -> [return: bb11, unwind continue]; }   // return Err(From::from(e))
 ... bb6–bb9 repeat the pattern for `b` ...
bb10: { _0 = Result::<u32, ParseIntError>::Ok(move _13); goto -> bb11; }
bb11: { return; }
```

(Trimmed; the variable declarations and scopes are omitted.) Every `?` is a `branch` call, a `switchInt` on
`ControlFlow`, and a `from_residual` call on the break edge. The `unwind continue` annotations are the panic paths from
Chapter 3.5. The early return is an ordinary edge to `bb11`, the function's single `return`, and any locals live at a
`?` get dropped on the way, exactly as on any other return.

> **What actually happens?** `?` is not an exception mechanism. It's sugar for a `match` and a `return`. After
> inlining, `branch` and `from_residual` are pattern matches on the same enum, and for identical error types the
> `From::from` call is the identity. §6 shows that nothing is left of them in release assembly.

**`main` returning `Result`.** [LIB] `main` may return any type implementing `Termination`. For `Result<(), E: Debug>`,
`Err(e)` prints `Error: {e:?}` to stderr and returns exit status 1 (`ExitCode::FAILURE`). `ExitCode` itself also
implements `Termination`, which is how `logstat` returns 2 for usage errors.

**Try blocks.** [VERSION] Nightly has `try { ... }` blocks, which scope `?` to a block instead of the whole function.
On stable, the idiom is an immediately-called closure (`(|| { ...; Ok(x) })()`) or a small helper function.

### 5. Memory

A `Result<T, E>` is an enum, so Chapter 2.6's layout rules apply. Its size is roughly `max(size_of T, size_of E)` plus a
tag, unless a **niche** (an invalid bit pattern in `T` or `E`) can hold the tag. Verified on rustc 1.98.1, x86-64
(listing `ch01-06-sizes.rs`):

```text
Result<(), ()>                                 1 bytes
Result<u64, ()>                               16 bytes
Result<&u64, ()>                               8 bytes
Result<NonZeroU32, ()>                         4 bytes
ParseIntError                                  1 bytes
Result<u32, ParseIntError>                     8 bytes
std::io::Error                                 8 bytes
Result<(), std::io::Error>                     8 bytes
Result<u64, std::io::Error>                   16 bytes
Box<dyn Error + Send + Sync>                  16 bytes
Result<(), Box<dyn Error + Send + Sync>>      16 bytes
anyhow::Error                                  8 bytes
Result<(), anyhow::Error>                      8 bytes
```

How to read it:

- **`Result<u64, ()>` = 16.** Every `u64` bit pattern is valid, so there's no niche. The tag needs its own byte, and
  alignment rounds it up to 8. This is the same result as Chapter 1.2's `Option<u64>`.
- **`Result<&u64, ()>` = 8** and **`Result<NonZeroU32, ()>` = 4.** A reference is never null, and `NonZeroU32` is never
  0, so `Err(())` is encoded as that forbidden value. [LANG] The null niche of references is guaranteed for `Option`.
  For other enums it's a [RUSTC] layout choice, observed here.
- **`ParseIntError` = 1 byte.** It's a wrapper around a small `IntErrorKind` enum. `Result<u32, ParseIntError>` is
  8 bytes, which §6 shows is returned in a single register.
- **`io::Error` = 8 bytes, and `Result<(), io::Error>` is also 8.** [LIB] On 64-bit targets, std represents
  `io::Error` as one tagged pointer: the low bits say whether it's an OS error code, a simple `ErrorKind`, a static
  message, or a boxed custom error. The value is never all-zero, so `Ok(())` fits in the niche. That's an implementation
  detail, not a guarantee, but it's why `io::Result<()>` costs one register.
- **`Box<dyn Error + Send + Sync>` = 16**: a fat pointer (data + vtable, Chapter 3.4). **`anyhow::Error` = 8**: [LIB]
  anyhow stores the vtable *inside* the heap allocation, so the handle is a thin pointer. Chapter 8.2 measures what
  each costs to create.

**The design consequence:** the error type's size is paid on **every** return, success or failure, because the caller
reserves space for the whole enum. A `Result<u64, E>` with a 512-byte `E` is a 520-byte return value even when it's
`Ok(5)`. Chapter 8.2 measures that and shows the assembly.

### 6. CPU / OS

**The happy path of `?`, in release assembly** (listing `ch01-07-happy-path.rs`, rustc 1.98.1, `-O`, x86-64). First,
how `parse_u32` returns its 8-byte `Result<u32, ParseIntError>`: **packed into `rax`**. Its return sites include:

```text
	shl	rax, 32          ; Ok(v): value in the high 32 bits, tag byte (al) = 0
	...
	mov	eax, 257         ; 0x0101: al = 1 (Err), ah = 1 (IntErrorKind::InvalidDigit)
	...
	mov	eax, 513         ; 0x0201: Err(PosOverflow)
	...
	mov	eax, 1           ; 0x0001: Err(Empty)
```

(The kind numbering is this build's layout [RUSTC], not a guarantee.) Now the caller with two `?`s:

```text
playground::sum_two:
	push	r15 / r14 / r12 / rbx / rax       ; (five pushes, condensed)
	mov	rbx, rcx
	mov	r14, rdx                          ; save b
	call	parse_u32                         ; x = parse_u32(a)
	mov	r15d, 1                           ; pre-load the Err tag
	test	al, 1                             ; ── first `?`: is it Err?
	jne	.LBB0_1                           ;    yes → propagate
	mov	r12, rax                          ; keep x (in the high half)
	mov	rdi, r14
	mov	rsi, rbx
	call	parse_u32                         ; y = parse_u32(b)
	test	al, 1                             ; ── second `?`
	je	.LBB0_4                           ;    Ok → go add
.LBB0_1:                                          ; ── error path (both `?`s share it)
	xor	ecx, ecx
	jmp	.LBB0_5
.LBB0_4:                                          ; ── happy path: wrapping_add in the high 32 bits
	movabs	rcx, -4294967296                  ; 0xFFFF_FFFF_0000_0000
	and	r12, rcx
	add	rax, r12                          ; the carry falls off the top: that's wrapping_add
	and	rax, rcx
	mov	rcx, rax
	xor	eax, eax
	xor	r15d, r15d                        ; tag = Ok
.LBB0_5:
	or	rcx, r15                          ; set the tag
	and	eax, 65280                        ; 0xFF00: keep the error-kind byte
	or	rax, rcx
	(pops) / ret
```

(Trimmed: the pushes and pops are condensed and the `@GOTPCREL` call syntax simplified; labels as emitted.) What `?`
costs on the happy path: **one `test` and one conditional jump per `?`**. The branch is almost never taken, so the CPU
predicts it correctly and it costs about a cycle. `branch`, `from_residual`, `From::from` and `ControlFlow` have all
disappeared. On the error path, propagation is three bitwise instructions: no allocation, no table lookup, no stack
walk.

**Contrast with exceptions.** [RUNTIME] Table-based exceptions (C++, and Rust's own panics, Chapter 8.3) are
"zero-cost" on the happy path: there's no `test` at all, only unwind tables that sit unused. The price is paid when one
is thrown: the unwinder looks up each frame in the tables, runs a personality routine, and walks the stack. Chapter 8.3
measures a panic plus `catch_unwind` at about 1.8 µs against 3 ns for returning an `Err` (one run, noisy). Java adds
the cost of `fillInStackTrace` at construction time. So:

| | Happy path | Failure path | Failure cost depends on |
|---|---|---|---|
| `Result` + `?` | a test + a predicted branch per call | same as happy path, plus error construction | size of `E` (copying) |
| Table-based exceptions / panics | nothing | microseconds | stack depth, table lookups |
| Java exceptions | nothing (after JIT) | stack-trace capture + unwinding | stack depth; the JIT may omit traces for hot built-in exceptions [RUNTIME] |

**When does this matter?** When failure is *common*: parsing untrusted input, cache misses modeled as errors, a
validation layer rejecting 30% of requests. There, `Result` has a predictable cost and exceptions don't. When failure
is rare, both are cheap. The rule of thumb this suggests (predicted from mechanism; the Systems exercise measures it):
**expected failures are values; bugs are panics.** Chapter 8.3 develops the second half.

---

## Pass 3 · Architect level — *Making failure part of the contract*

### 7. Trade-offs

**`Option` vs `Result<T, ()>` vs `Result<Option<T>, E>`.**

| Signature | Meaning | Example |
|---|---|---|
| `Option<T>` | May be absent; absence is normal | `cache.get(k)` |
| `Result<T, E>` | Must be present; failure has reasons | `parse_config(text)` |
| `Result<Option<T>, E>` | Lookup that can *fail*, and whose answer can be "not there" | `db.find_user(id)`: `Ok(None)` = no such user, `Err(_)` = the database is down |
| `Option<Result<T, E>>` | Rare; used by iterators of fallible items (`Iterator<Item = Result<..>>`) | `lines.next()` on a reader |
| `Result<T, ()>` | Almost always a smell: use `Option`, or give the error a name | |

The `Result<Option<T>, E>` row prevents a classic outage bug: treating "the database is down" as "the user doesn't
exist" and creating a duplicate account. Java's `Optional<User> findUser(id)` that returns `Optional.empty()` on a
`SQLException` has exactly that bug.

**`unwrap`, `expect`, and `?`.** A team policy that works:

- `?` whenever the enclosing function can report the failure.
- `expect("why this can't fail")` when a failure would be a **bug** and you can state the invariant: e.g.
  `head[4..8].try_into().expect("exactly 4 bytes")` in Chapter 2.4. The message is written for the future reader of a
  panic message, not the user.
- `unwrap()` in tests, examples, and prototypes. In production code review, every `unwrap()` on anything derived from
  input is a finding (the Part VIII review has one).
- `unwrap_or` / `unwrap_or_default` only when the default is **correct**, not merely convenient. §9 is the
  counterexample.

**Combinators or `match`?** `map`, `map_err`, `and_then`, `ok_or_else`, `unwrap_or_else` read well in short chains.
Beyond two or three links, a `match` or `let ... else` is clearer, and it's easier to set a breakpoint in. All compile to
the same code.

> **Why not exceptions?** Three reasons, each architectural. **Visibility:** with exceptions, any call might exit the
> function, so reviewers can't see the exit paths, and resource cleanup needs `finally` or try-with-resources at every
> site. **Types:** the set of possible failures isn't in the signature (except for checked exceptions, see §8), so
> callers learn about them in production. **Cost model:** throwing is expensive and unpredictable, so exceptions can't
> be used for common outcomes, and APIs grow two versions (`parseInt` vs `tryParse`). Rust's `Result` puts failures in the
> signature, makes every exit visible (`?`), and costs the same whichever way it goes.

### 8. Java comparison

| Java / Go | Rust | Notes |
|---|---|---|
| Checked exception (`throws IOException`) | `Result<T, io::Error>` | Both put failure in the signature. Checked exceptions don't compose with lambdas and streams (`Function` can't throw), so Java code wraps them in unchecked ones. `Result` is a value, so it flows through closures, iterators, and futures unchanged. |
| Unchecked exception (`NumberFormatException`) | `Result` from `parse`, or a panic for bugs | Java makes expected failures invisible. Rust makes them values and reserves panics for bugs (Chapter 8.3). |
| `Optional<T>` | `Option<T>` | `Optional` is an object: it can itself be `null`, it allocates (unless escape analysis removes it), and Java's style guides discourage it for fields and parameters. `Option<&T>` is the size of a pointer (§5) and used everywhere. |
| `null` returns | `Option<T>` | The compiler forces the check. |
| Go `val, err := f()` + `if err != nil { return err }` | `let val = f()?;` | Go returns *both* a value and an error; nothing stops you from using `val` when `err != nil`. `Result` holds exactly one. |
| Go `fmt.Errorf("...: %w", err)` | `map_err` / `From` / `.context()` (Chapter 8.2) | Wrapping with a cause chain. |

> **Analogy limit.** "`?` is Java's `throws` clause" captures propagation and stops there. `?` returns from **this**
> function only. It's a local, visible return with an explicit conversion (`From`), not a non-local jump that searches
> the stack for a matching `catch`. There's no catch-by-type up the stack: each caller receives a value and decides.
> Resource cleanup is `Drop` on the ordinary return path (Chapter 3.5), not a separate `finally` mechanism. And the cost
> is a branch, not a stack walk.

### 9. Production scenario

**The timeout that silently wasn't.** Meridian's Java gateway reads about forty settings from the environment at
startup. One of them was read like this:

```java
int timeoutMs;
try {
    timeoutMs = Integer.parseInt(System.getenv().getOrDefault("UPSTREAM_TIMEOUT_MS", "1000"));
} catch (NumberFormatException e) {
    timeoutMs = 1000; // "safe default"
}
```

During a 2025 incident, an operator lowered the upstream timeout to shed load, setting `UPSTREAM_TIMEOUT_MS=250ms`
(with a unit). `parseInt` threw, the `catch` swallowed it, and the gateway kept the 1,000 ms default for the whole
incident. The dashboards showed the new value in the deployment config, so nobody suspected the setting for 40 minutes.

The bug is that **two different situations were merged**: "the setting is absent" (a default is correct) and "the
setting is present but malformed" (the operator made a mistake, so the process should refuse to start and say why). When
the Rust gateway's config loader was written, the team encoded the distinction in types, exactly as `timeout_ms` does in
§3:

```text
find_timeout(config)  →  Option<&str>        None: absent → default 1000 (correct and intended)
parse_timeout(raw)    →  Result<u64, E>      Err: malformed → startup fails with the key and the raw value
```

The verified output from §3 is the error an operator now sees:

```text
    bad: Err("timeout_ms=\"25O\": invalid digit found in string")
```

The general rule for configuration: **absence may default; malformed never does.** And fail at startup, not on the first
request that reads the value. Parse all configuration into a typed struct before the server binds its port (Chapter 2.6's
"parse, don't validate").

### 10. Failure scenario

**The lock file that was never released.** Meridian's nightly settlement job is a Rust binary that takes a lock file,
does its work, and removes the lock. The removal was written as in listing `ch01-08-must-use.rs`: the `Result` of
`fs::remove_file` was ignored. The compiler warned (§3), but the job's crate had over 300 warnings, and nobody read
them.

One night the job ran under a different service account after a migration. It could create the lock but not delete it,
because the directory's permissions differed. `remove_file` returned `Err(PermissionDenied)`, the job logged "lock
released", and exited 0. The next night's run saw the lock, assumed a concurrent run, and refused to start. Settlement
was a day late, and the logs said everything was fine.

What went wrong, layer by layer:

- **Code:** a fallible cleanup ignored its `Result`. The fix is to handle it: `fs::remove_file(path)?`, or, if the job
  should still succeed, log it at ERROR with the path and error and emit a metric.
- **Build policy:** warnings were noise. Meridian's Rust CI now builds with `RUSTFLAGS="-D warnings"`, and
  `unused_must_use` specifically is `deny` in every crate root (`#![deny(unused_must_use)]`).
- **Design:** a lock that relies on the holder to clean up is fragile anyway. Chapter 3.5's design exercise (leases with
  server-side expiry) is the robust version.

`let _ = ...` is not a fix. It turns off the warning and keeps the bug. In review, `let _ =` on a `Result` needs a
comment explaining why the error doesn't matter.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part VIII).*

1. When should a function return `Option<T>`, when `Result<T, E>`, and when `Result<Option<T>, E>`? Give a production
   example of the bug that the last one prevents.
2. Desugar `let x = f()?;` for a function returning `Result<T, MyError>`. Where does the error conversion happen, and
   which trait bound must hold?
3. Why does `?` on an `Option` inside a function returning `Result` fail to compile? What are two ways to fix it?
4. Why are both of `?`'s common errors E0277?
5. `size_of::<Result<(), io::Error>>()` is 8 on x86-64. Explain why, and say which parts of that explanation are
   guarantees and which are implementation details.
6. What does `?` cost on the happy path in release builds? What does a thrown exception cost? When does the difference
   matter architecturally?
7. What does `fn main() -> Result<(), E>` print on error, and with which formatting trait? Why is that a poor fit for a
   user-facing CLI?
8. A reviewer says "just `unwrap_or_default()` it". When is that correct, and when is it the §9 bug?

### 12. Exercises

- **Beginner.** Write `fn parse_kv(line: &str) -> Option<(&str, &str)>` that splits `"key=value"` at the first `=`, and
  `fn parse_port(line: &str) -> Result<u16, String>` that uses it and reports "missing `=`", "wrong key", or the parse
  error. Use `?` at least twice.
- **Intermediate.** Parse a list of `"key=value"` lines into `HashMap<String, u32>`, using `collect::<Result<_, _>>()` so
  that the first bad line stops the parse. Then change it to collect *all* errors instead of stopping at the first.
  Which do users of a config file prefer?
- **Advanced.** Write a function returning `Result<Option<u32>, ConfigError>` for an optional numeric setting, using
  `Option::map` + `transpose`. Then write the same with `match`. Which is clearer to a reviewer who's new to Rust?
- **Systems.** Emit release assembly for `sum_two` on the Playground. Change `ParseIntError` to a 64-byte error type and
  find where the `Result` is returned now (register or memory?). Then time 10 million calls where 50% fail, using
  `Result`, and compare with a version that panics and uses `catch_unwind`. Report one run and say why it's noisy.
- **Architecture.** Take a Java service you know and list five methods that throw unchecked exceptions for *expected*
  outcomes (not found, invalid input, conflict). Write the Rust signature for each, including the error enum.

### 13. Debugging exercise

A teammate writes this (listing `ch01-09-option-in-result.rs`):

```rust,compile_fail
use std::collections::HashMap;
use std::num::ParseIntError;

fn pool_size(env: &HashMap<String, String>) -> Result<u32, ParseIntError> {
    let raw = env.get("POOL_SIZE")?;
    raw.parse()
}
```

```text
error[E0277]: the `?` operator can only be used on `Result`s, not `Option`s, in a function that returns `Result`
 --> src/main.rs:6:35
  |
5 | fn pool_size(env: &HashMap<String, String>) -> Result<u32, ParseIntError> {
  | ------------------------------------------------------------------------- this function returns a `Result`
6 |     let raw = env.get("POOL_SIZE")?;
  |                                   ^ use `.ok_or(...)?` to provide an error compatible with `Result<u32, ParseIntError>`
```

1. Explain the error in terms of §4's traits: which `FromResidual` impl would be needed, and why doesn't std provide it?
2. The compiler suggests `.ok_or(...)?`. What value could you pass, given that the error type is `ParseIntError`? Why is
   that a dead end, and what does it tell you about the function's error type?
3. Redesign the signature twice: once where a missing `POOL_SIZE` is an error, and once where it defaults to 16. Which
   one does §9's rule recommend, and does your answer depend on the setting?

### 14. Design exercise

**Meridian's typed configuration.** Design the configuration API for the Rust gateway: about forty settings, from
environment variables and a file, some required, some optional with defaults, some that must be parsed as durations
(`"250ms"`, `"2s"`), and a few secrets.

- What's the signature of the generic accessor? Compare `fn get<T: FromStr>(&self, key) -> Result<Option<T>, ConfigError>`
  with `fn get_or<T>(&self, key, default: T) -> Result<T, ConfigError>` and with deriving the whole struct at once.
- Should the loader stop at the first error or report all of them? Who is the audience for the error message?
- What must never appear in the error message? (Hint: one of the settings is a database password.)
- Where in the process lifecycle does parsing happen, and what does the process do on failure?
