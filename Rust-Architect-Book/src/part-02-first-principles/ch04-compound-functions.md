# Chapter 2.4 — Compound Types, Functions, Expressions, and Statements

> **Where this sits:** Part II · Rust From First Principles · chapter 4 of 7
> **Prerequisites:** Chapter 2.3.
> **After this chapter you can:** write expression-oriented Rust fluently; use tuples, arrays, `()`, and `!` precisely;
> explain how arguments and return values travel between functions at the machine level; tell function items, function
> pointers, and closures apart; and avoid the large-array stack overflow that hits every systems team once.

---

## Pass 1 · User level — *Expressions everywhere*

### 1. Problem

Java is statement-oriented. `if` is a statement, so you declare a variable first and assign it in each branch, or you
reach for the ternary. `switch` only became an expression in Java 14. Rust is **expression-oriented**: blocks, `if`,
`match`, and `loop` all produce values. That changes how code is shaped, with fewer mutable temporaries and fewer "declare
now, assign later" variables.

Rust's compound types are also **values with sizes known at compile time**. A tuple or an array can live on the stack
with no header and no allocation. That's excellent for performance, and it's also how you overflow a 2 MiB thread stack
without writing any recursion. And every function signature is a fully typed contract. Since Chapter 2.3 showed that
inference stops at signatures, the signature is where APIs are designed.

### 2. Mental model

```text
 EXPRESSION   produces a value:   1 + 2    if c { a } else { b }    { let t = f(); t * 2 }    loop { break 7 }
 STATEMENT    produces nothing:   let x = ...;     fn helper() {}      expr;   ← an expression turned INTO a statement
                                                                                 (the `;` discards its value)

 A BLOCK's value is its TAIL expression, the last one, written WITHOUT a semicolon.
 No tail expression → the block's value is ()   (unit: the type with exactly one value)
 An expression that never produces a value (panic!, return, break, continue, loop {}) has type !   ("never")
```

The compound types:

| Type | What it is | Size | Example |
|---|---|---|---|
| `(A, B, C)` | Tuple: an anonymous product type | Fields plus padding | `(lo, hi)` returned from a function |
| `[T; N]` | Array: `N` elements, **`N` is part of the type** | `N × size_of::<T>()` | `[u8; 16]`, a fixed header |
| `[T]` | Slice: a run of `T` whose length is known only at run time | Unsized | used through `&[T]`, `&mut [T]` |
| `&[T]` | A *reference* to a slice: pointer + length | 2 words | a view into an array or a `Vec` |
| `()` | Unit | 0 | the return type of "procedures" |
| `!` | Never | n/a (no values) | the type of `panic!()` |

### 3. Rust code

Expression orientation in practice (verified):

```rust
fn classify(latency_ms: u32) -> &'static str {
    // `if` is an expression; every branch must produce the same type.
    if latency_ms < 100 {
        "fast"
    } else if latency_ms < 1_000 {
        "slow"
    } else {
        "timeout-risk"
    }
}

fn first_over(limit: u32, samples: &[u32]) -> Option<usize> {
    let mut i = 0;
    // `loop` is an expression too: `break value` produces its result.
    loop {
        if i == samples.len() {
            break None;
        }
        if samples[i] > limit {
            break Some(i);
        }
        i += 1;
    }
}

fn min_max(samples: &[u32]) -> (u32, u32) {
    // A block evaluates to its tail expression (the last line, without a semicolon).
    let min = {
        let mut m = u32::MAX;
        for &s in samples {
            m = m.min(s);
        }
        m
    };
    let max = samples.iter().copied().max().unwrap_or(0);
    (min, max) // a tuple: an anonymous product type
}

fn main() {
    let samples = [120, 45, 3_000, 80]; // [u32; 4]: the length is part of the type
    for s in samples {
        println!("{s:>5} ms -> {}", classify(s));
    }
    let (lo, hi) = min_max(&samples); // destructuring the tuple
    println!("min={lo} max={hi}");
    println!("first over 1000 at index {:?}", first_over(1_000, &samples));

    // A labeled block: `break 'label value` exits it with a value.
    let grid = [[1, 2, 3], [4, 99, 6]];
    let pos = 'search: {
        for (r, row) in grid.iter().enumerate() {
            for (c, &v) in row.iter().enumerate() {
                if v == 99 {
                    break 'search Some((r, c));
                }
            }
        }
        None
    };
    println!("99 found at {pos:?}");

    // `!` (never): panic!, return, continue, and `loop {}` produce no value, so they fit any type.
    let parsed: u32 = match "17".parse() {
        Ok(n) => n,
        Err(_) => panic!("not a number"),
    };
    println!("parsed={parsed}");
}
```

```text
  120 ms -> slow
   45 ms -> fast
 3000 ms -> timeout-risk
   80 ms -> fast
min=45 max=3000
first over 1000 at index Some(2)
99 found at Some((1, 1))
parsed=17
```

Notice what's missing: no `return` statements (the tail expression is the return value), no "declare `result`, assign
it in each branch," and the `match` arm `panic!(...)` sits happily next to an arm of type `u32` because `!` fits any type.

Functions come in three flavors that look alike at the call site and are very different underneath (verified sizes):

```rust
use std::mem::size_of_val;

fn double(x: u64) -> u64 {
    x * 2
}

fn main() {
    let item = double; // a function ITEM: a unique, zero-sized type
    let ptr: fn(u64) -> u64 = double; // a function POINTER: an address
    let factor = 3;
    let closure = move |x: u64| x * factor; // a closure: a struct holding its captures
    println!("fn item:    {} bytes", size_of_val(&item));
    println!("fn pointer: {} bytes", size_of_val(&ptr));
    println!("closure:    {} bytes (captures one u64)", size_of_val(&closure));
    println!("unit ():    {} bytes", size_of_val(&()));
    println!("results:    {} {} {}", item(21), ptr(21), closure(14));
}
```

```text
fn item:    0 bytes
fn pointer: 8 bytes
closure:    8 bytes (captures one u64)
unit ():    0 bytes
results:    42 42 42
```

A function *item* is zero bytes because its identity is carried entirely by its **type**. Each function has its own
unique type, so the compiler knows statically which code a call goes to. §4 explains why that matters.

---

## Pass 2 · Systems level — *How values travel between functions*

### 4. Under the hood

**Expressions don't cost anything extra.** Expression orientation is a *source-level* property. `if c { a } else { b }`
lowers to the same control-flow graph as a statement-style `if` with assignments: branches that merge with a φ node in
SSA (Chapter 2.3), often turned into a branchless `cmov` or `select`. `loop { ... break v }` becomes basic blocks where
`v` flows into a φ at the exit. There's no runtime representation of "an expression."

**How arguments and results travel: the calling convention.** [LANG] The Rust ABI (`extern "Rust"`, the default) is
**unspecified and unstable**. rustc may pass arguments however it likes and may change that between versions, which is
one reason Rust has no stable binary interface for plugins (Part XVI uses `extern "C"` for that). [RUSTC] In practice,
on x86-64 Linux it looks a lot like the C convention. Here are two functions, compiled in release mode by rustc 1.98.1:

```rust,ignore
#[inline(never)]
pub fn split(x: u64) -> (u64, u64) {
    (x >> 32, x & 0xFFFF_FFFF)
}

#[inline(never)]
pub fn table(seed: u64) -> [u64; 8] {
    let mut t = [0u64; 8];
    for i in 0..8 {
        t[i] = seed.wrapping_mul(i as u64 + 1);
    }
    t
}
```

```text
playground::split:                     ; x arrives in rdi
	mov	edx, edi                ; second tuple field → rdx   (x & 0xFFFF_FFFF: a 32-bit mov zero-extends)
	mov	rax, rdi
	shr	rax, 32                 ; first tuple field  → rax   (x >> 32)
	ret                             ; a two-word value comes back in the register PAIR rax:rdx

playground::table:                     ; rdi = a HIDDEN pointer to the caller's 64-byte result slot
	mov	rax, rdi                ; rsi = seed
	mov	qword ptr [rdi], rsi    ; t[0] = seed * 1
	lea	rcx, [rsi + rsi]
	mov	qword ptr [rdi + 8], rcx      ; t[1] = seed * 2
	lea	rdx, [rsi + 2*rsi]
	mov	qword ptr [rdi + 16], rdx     ; t[2] = seed * 3
	lea	rdx, [4*rsi]
	mov	qword ptr [rdi + 24], rdx     ; t[3] = seed * 4
	lea	rdx, [rsi + 4*rsi]
	mov	qword ptr [rdi + 32], rdx     ; t[4] = seed * 5
	lea	rcx, [rcx + 2*rcx]
	mov	qword ptr [rdi + 40], rcx     ; t[5] = seed * 6  (= 3 × t[1])
	lea	rcx, [8*rsi]
	mov	rdx, rcx
	sub	rdx, rsi
	mov	qword ptr [rdi + 48], rdx     ; t[6] = seed * 7  (= 8·seed − seed)
	mov	qword ptr [rdi + 56], rcx     ; t[7] = seed * 8
	ret                                   ; rax = the same pointer, by convention
```

Three lessons are packed into that listing:

1. **Small aggregates come back in registers.** The `(u64, u64)` tuple never touches memory: `rax` carries one field
   and `rdx` the other.
2. **Large aggregates are returned through a hidden pointer** (LLVM calls it `sret`). The caller reserves 64 bytes and
   passes their address in `rdi`, and the callee writes **directly into the caller's memory**. Returning a big value "by
   value" is therefore *not* a copy. It's the pattern C++ calls return-value optimization, and in Rust it's simply how
   the ABI works. [RUSTC] This isn't a language guarantee, but it's reliable in practice.
3. **The optimizer rewrote the program.** The loop is gone, fully unrolled, and all eight multiplications became `lea`,
   shift, and subtract instructions (strength reduction). `t` never existed as a local array. The function writes its
   final values straight into the caller's slot.

**Function items vs pointers vs closures.** A call through a function **item** is a *direct* call to a known address,
so the compiler can inline it. A call through a function **pointer** is an *indirect* call: `call rax`, a jump to
whatever address the pointer holds. The optimizer can inline it only if it can prove what the pointer contains. A
**closure** is an anonymous struct holding its captures (here, `factor`), with its own unique type, just like a function
item, so calling it is direct and inlinable too. This distinction is the seed of Rust's static-vs-dynamic dispatch story
(Part VI) and of why iterator chains with closures optimize so well (Part X).

**The `!` type.** [LANG] An expression of type `!` never completes. The compiler can coerce `!` to any type, which is
why `Err(_) => panic!(...)` type-checks next to `Ok(n) => n`, and it knows the code after is unreachable. `loop {}`
without a `break` has type `!`, and so do `return`, `break`, `continue`, and `std::process::exit`.

### 5. Memory

```text
 (u32, u8, u64)        [RUSTC] tuple fields may be REORDERED to reduce padding; no layout guarantee
 [u32; 4]              [LANG]  contiguous, no header, no stored length; stride = size_of::<u32>()
 ┌────┬────┬────┬────┐          the length 4 lives in the TYPE, not in memory
 │120 │ 45 │3000│ 80 │   16 bytes, on the stack if it's a local
 └────┴────┴────┴────┘

 &[u32] (a "fat pointer")        ┌──────────┬──────────┐
                                 │ ptr ─────┼──► into the array above (or into a Vec's heap buffer)
                                 │ len = 4  │
                                 └──────────┴──────────┘   16 bytes: the length travels WITH the reference
```

An array is a *value*. `let b = a;` copies all its bytes (if `T: Copy`), and passing `[u8; 4096]` by value means moving
4 KiB. [RUSTC] The optimizer often elides such copies, but they're allowed to exist. The **slice reference** `&[T]` is
how functions accept "any contiguous run of `T`": part of an array, all of a `Vec`, a subrange of either. That's why
`min_max(&samples)` takes `&[u32]` rather than `[u32; 4]` (Part III covers slices in depth).

**Large arrays and the stack.** A local array lives in the function's stack frame. [LIB] Spawned threads get a
**2 MiB** stack by default, and [OS] the main thread's stack is set by the OS (commonly 8 MiB on Linux via
`ulimit -s`). So this crashes:

```rust
use std::hint::black_box;
use std::thread;

fn checksum(buf: &[u8]) -> u64 {
    buf.iter().map(|&b| b as u64).sum()
}

fn main() {
    // Spawned threads get a 2 MiB stack by default; this array alone is 4 MiB.
    let handle = thread::spawn(|| {
        let buf = [1u8; 4 * 1024 * 1024];
        checksum(black_box(&buf))
    });
    println!("checksum = {}", handle.join().unwrap());
}
```

```text
thread '<unknown>' (45) has overflowed its stack
fatal runtime error: stack overflow, aborting
```

This is an **abort**, not a panic: no unwinding, no `catch_unwind`, and the whole process dies (Chapter 1.3's guard
page at work). The fix is to put the buffer on the heap, which leaves a 24-byte handle on the stack:
`let buf = vec![1u8; 4 * 1024 * 1024];` (verified: prints `checksum = 4194304`). Be careful with
`Box::new([0u8; N])` too. [RUSTC] The array may be built on the stack *first* and then moved to the heap, especially in
debug builds. `vec![0u8; N]` (or `vec![...].into_boxed_slice()`) allocates directly.

### 6. CPU / OS

- **A call** pushes the return address and jumps. A **prologue** may adjust `rsp` for locals and save callee-saved
  registers, and the **epilogue** undoes it. The x86-64 System V ABI requires `rsp` to be 16-byte aligned at each call.
  Leaf functions (ones that call nothing) may use the 128-byte **red zone** below `rsp` without adjusting it. That's why
  the release `split` and `table` have no frame at all.
- **Inlining removes calls entirely.** Most small Rust functions never exist as calls in a release binary. `#[inline]`
  is a hint (and makes a function's body available to other crates, Chapter 2.7). `#[inline(never)]`, used in this
  book's listings, forces a real call so we can look at it.
- **No guaranteed tail calls.** [LANG] Rust doesn't promise tail-call elimination. LLVM *may* turn a tail-recursive call
  into a loop in release builds, but that's an optimization, not a guarantee. (The `become` keyword is reserved for a
  possible explicit tail-call feature; it's unstable.) Deep recursion over untrusted input, such as a nested JSON
  document or a deep tree, can overflow the stack. The systems answers are iteration with an explicit stack (Part IX's
  interlude on BFS vs DFS) or a hard depth limit.

---

## Pass 3 · Architect level — *Choosing shapes for data and APIs*

### 7. Trade-offs

**Array vs `Vec` vs slice:**

| | `[T; N]` | `Vec<T>` | `&[T]` / `&mut [T]` |
|---|---|---|---|
| Where the elements live | Inline (stack, or inside the containing struct) | Heap | Wherever the owner put them |
| Length | Compile-time constant | Run-time, can grow | Run-time, fixed for the view |
| Allocation | None | On creation and growth | None (a view) |
| Ownership | Owns | Owns | Borrows |
| Use for | Fixed-size headers, keys, small lookup tables, SIMD lanes | Collections of unknown or large size | **Function parameters**: accept any contiguous data |
| Risk | Large `N` overflows the stack | Allocation cost; reallocation (Chapter 1.1) | Lifetimes (Part IV) |

**Tuple vs struct.** Tuples are fine for *local, short-lived* groupings, like a private helper returning `(min, max)`.
Anything public, stored, or passed through several layers should be a named `struct`: `(u32, u32)` doesn't tell you
which field is min, while `Range { min, max }` does. Neither is faster; the layout is the same kind.

**Small `Copy` values: pass by value.** A `u64` or a two-field `Copy` struct fits in registers. Passing `&u64` instead
adds a pointer indirection and makes the value's address observable. Pass references for large values or when you need
to borrow instead of copy.

**Returning large values:** return them by value. The hidden-pointer ABI makes it efficient, and it keeps ownership
clear. The "fill this `&mut` output buffer" style is for *reusing* a buffer across many calls (Project Level 1 does
exactly that), not for avoiding a copy.

### 8. Java comparison

| Java | Rust | Note |
|---|---|---|
| `int[]`: a heap object with a header and a length field | `[i32; N]` (inline, no header) or `Vec<i32>` (heap) | A Java array is always a reference to a heap object. A Rust array is the bytes themselves. |
| No tuples (records since Java 16; `Map.Entry`; `Pair` libraries) | Built-in tuples | Java records are *nominal* tuples, so the Rust analog is a tuple struct or a named struct. |
| `switch` expressions with `yield` (Java 14) | `match` and `if` are always expressions | The ternary `c ? a : b` is just `if c { a } else { b }` in Rust. |
| `void` | `()` | `()` is a **real type with a value**, which enables `Result<(), E>`, `HashMap<K, ()>` as a set, and generic code that doesn't special-case "no value." |
| No equivalent (Kotlin has `Nothing`) | `!` | The type checker knows a branch diverges. |
| Methods live in classes; lambdas are objects implementing an interface | Free functions, methods, and closures (zero-sized or struct-sized, with a unique type) | A Java lambda call goes through an interface (`invokeinterface`), and the JIT often devirtualizes it. A Rust closure call is static by construction. |

> **Analogy limit.** "`()` is `void`" holds for "returns nothing useful." It fails in generic code. Java has to
> special-case `void` (you can't write `List<void>`, so there's `Void` and `null`). In Rust `()` is an ordinary type, so
> `HashMap<K, ()>` is literally how `HashSet<K>` is implemented, at zero bytes per value.

### 9. Production scenario

**Meridian's binary protocol header.** The market-data fan-out receives frames of `magic | version | flags |
body length (big-endian) | body`. The parser must allocate nothing per frame, since there are millions per second:

```rust
/// Wire header: magic (2 bytes) | version (1) | flags (1) | body length (4, big-endian) = 8 bytes.
#[derive(Debug)]
struct Header {
    version: u8,
    flags: u8,
    body_len: u32,
}

const MAGIC: [u8; 2] = [0xCA, 0xFE];

fn parse_header(buf: &[u8]) -> Result<(Header, &[u8]), String> {
    if buf.len() < 8 {
        return Err(format!("need 8 header bytes, got {}", buf.len()));
    }
    let (head, rest) = buf.split_at(8);
    if head[0..2] != MAGIC {
        return Err(format!("bad magic {:#04x} {:#04x}", head[0], head[1]));
    }
    let len_bytes: [u8; 4] = head[4..8].try_into().expect("exactly 4 bytes");
    let header = Header { version: head[2], flags: head[3], body_len: u32::from_be_bytes(len_bytes) };
    Ok((header, rest))
}

fn main() {
    let frame = [0xCA, 0xFE, 1, 0b0000_0010, 0, 0, 0, 5, b'h', b'e', b'l', b'l', b'o'];
    match parse_header(&frame) {
        Ok((h, body)) => println!(
            "version={} flags={:#04b} body_len={} body={:?}",
            h.version, h.flags, h.body_len, std::str::from_utf8(body)
        ),
        Err(e) => println!("error: {e}"),
    }
    println!("{:?}", parse_header(&frame[..5]).map(|(h, _)| h.body_len));
    println!("{:?}", parse_header(&[0xBA, 0xAD, 1, 0, 0, 0, 0, 0]).map(|(h, _)| h.body_len));
}
```

```text
version=1 flags=0b10 body_len=5 body=Ok("hello")
Err("need 8 header bytes, got 5")
Err("bad magic 0xba 0xad")
```

The design points: the input is a **slice** (so the caller can pass a network buffer, part of a buffer, or an array);
the length check comes *first*, so every index after it is in bounds; `try_into()` turns a 4-byte slice into a
`[u8; 4]` array, whose length is part of its type; and the body is returned as a **sub-slice of the input** with no copy
(how the compiler knows that returned slice can't outlive `buf` is Part IV). The `String` error type is a placeholder;
Part VIII replaces it with a proper error enum. Chapter 2.5 rewrites this parser with slice patterns.

### 10. Failure scenario

**The compression buffer that worked in tests.** Meridian's export service compresses reports. A developer declares
the scratch buffer as a local array:

```rust,ignore
fn compress_report(report: &[u8]) -> Vec<u8> {
    let mut scratch = [0u8; 4 * 1024 * 1024]; // "stack allocation is fast"
    ...
}
```

The developer benchmarks it from a small command-line harness that calls it on the **main thread**. [OS] On Linux that
stack is typically 8 MiB, so everything works, and the function has no unit tests (which would have run on the test
harness's spawned threads and crashed). In production the function runs on a thread-pool worker with a 2 MiB stack. The first real report crashes the process with
`has overflowed its stack` / `fatal runtime error: stack overflow, aborting`. It's an abort, so every in-flight export
on that instance is lost. Autoscaling restarts it, the next report kills it again, and the service crash-loops.

**Fixes, in order of preference:** a heap buffer allocated **once per worker and reused** (no per-call allocation, no
stack risk); or `vec![0u8; N]` per call if allocation cost doesn't matter; or, as a last resort, a larger stack for
those workers via `std::thread::Builder::stack_size`. The review rule Meridian adopted: **no local arrays over a few KB
in code that can run on non-main threads.**

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part II).*

1. What exactly does a semicolon do in Rust? Why does ending a function with `sum;` cause E0308?
2. `()` vs Java's `void`: why is unit a real type, and what does that make possible?
3. What is the `!` type? Why can `panic!()` appear in a `match` arm whose other arms produce `u32`?
4. How are a `(u64, u64)` and a `[u64; 8]` returned from a function on x86-64? Is returning large values by value
   expensive?
5. Why is a function item zero-sized, and what does that have to do with inlining and static dispatch?
6. Compare `[T; N]`, `Vec<T>`, and `&[T]`: memory layout, where the length lives, and when to use each.
7. Why doesn't Rust guarantee tail-call elimination, and what should you do instead of deep recursion on untrusted
   input?
8. A 4 MiB array works in `main` but crashes in a spawned thread. Explain exactly why, and why it's an abort rather than
   a panic.

### 12. Exercises

- **Beginner.** Rewrite this Java-style Rust in expression style, with no `mut` and no `return`:
  `fn grade(score: u32) -> char { let mut g = 'F'; if score >= 90 { g = 'A'; } else if score >= 75 { g = 'B'; } return g; }`
- **Intermediate.** Write `fn stats(xs: &[f64]) -> Option<(f64, f64, f64)>` returning (min, max, mean), then refactor it
  to return a named struct. Which version would you put in a public API, and why?
- **Advanced.** Using the Playground's ASM output (release, `#[inline(never)]`), find the size at which a returned
  struct of `u64` fields switches from registers to a hidden pointer. Try 1, 2, 3, and 4 fields.
- **Systems.** On Linux, check `ulimit -s`. Write a recursive function with a 1 KiB local array and find the depth at
  which it overflows on the main thread and on a spawned thread, in debug and release builds. Explain every difference.
- **Architecture.** Write team guidelines covering large buffers, recursion depth on untrusted input, and thread stack
  sizes, for services that run work on thread pools.

### 13. Debugging exercise

```rust,compile_fail
fn total_cents(items: &[u64]) -> u64 {
    let mut sum = 0;
    for item in items {
        sum += item;
    }
    sum;
}

fn main() {
    println!("{}", total_cents(&[250, 199]));
}
```

```text
error[E0308]: mismatched types
 --> src/main.rs:2:34
  |
2 | fn total_cents(items: &[u64]) -> u64 {
  |    -----------                   ^^^ expected `u64`, found `()`
  |    |
  |    implicitly returns `()` as its body has no tail or `return` expression
...
7 |     sum;
  |        - help: remove this semicolon to return this value
```

1. Explain the error using the words *tail expression*, *statement*, and *unit*.
2. Why does the compiler blame the **return type** on line 2 and not line 7?
3. After fixing it, rewrite the body as a single expression with no `mut` (hint: `iter().sum()`). Then check the release
   assembly of both versions. Are they different?

### 14. Design exercise

**API shape for Meridian's frame parser.** The `parse_header` function above is going into a shared library used by
four services. Decide:

- Should it return `(Header, &[u8])`, a `Frame<'a> { header, body: &'a [u8] }` struct, or an owned `Frame` with a
  `Vec<u8>` body? Consider callers that process frames immediately and callers that queue them to another thread.
- Should it take `&[u8]` or some "buffer" abstraction that can hold a partial frame across reads from a TCP socket?
- What should happen when `body_len` claims more bytes than have arrived: an error, or "need more data"?

Write the signatures, then justify them on allocation, ownership, and how each option handles a frame split across two
network reads. (Part XIII's async server and Part XXI's networking chapters will come back to your answer.)
