# Chapter 7.1 — Generics from Call Site to Binary

> **Where this sits:** Part VII · Generics and Monomorphization · chapter 1 of 3
> **Prerequisites:** Chapter 1.3 (the iterator-vs-loop assembly), Chapter 2.1 (what an `.rlib` ships), Chapter 4.3
> (lifetime parameters are erased). Chapter 6.1, *Traits, Bounds, and Default Methods*, is useful but not required.
> **After this chapter you can:** follow `fn process<T>(x: T)` from source to machine code at four levels; predict how
> many copies of a generic function a program contains and what each one looks like; read v0 symbol names; explain why
> Rust reports a missing bound at the definition (unlike C++ templates) and name the one exception; use `where` clauses,
> turbofish, const generics, and `impl Trait` arguments deliberately; and explain Chapter 1.3's identical assembly.

---

## Pass 1 · User level — *One definition, many functions*

### 1. Problem

You write an algorithm once and want it to work for many types: `largest` over `u8`, `i64`, and `f64`; a parser that
yields `u16` for one config key and `f64` for another; a line counter that reads stdin in production and a byte slice in
tests. Every language with static types answers this, and the answers differ a lot in what they cost:

- **Java** compiles one body that works on `Object` references and inserts casts at the call sites (type erasure,
  Chapter 7.2). Primitives must be boxed to take part.
- **C++** stamps out a separate copy of a template for each type, and type-checks each copy only when it is stamped out.
- **Go** (since 1.18) compiles one copy per *GC shape* and passes a dictionary for the type-specific parts (Chapter 1.2's
  table).
- **Rust** type-checks the generic body **once**, against its declared bounds, then generates a separate copy for each
  concrete type that is actually used. This is called **monomorphization**.

An architect needs to answer five questions about any generic function: *How many copies exist? What does each copy look
like? Where are type errors reported? What does it cost in binary size and compile time? And when should it not be
generic at all?* This chapter answers the first three. Chapter 7.2 contrasts the model with Java's, and Chapter 7.3 covers
the costs.

### 2. Mental model

**A generic function is a checked template: checked once, stamped per type.**

```text
  SOURCE (you write one)                       WHAT THE COMPILER BUILDS (one per concrete T actually used)

  fn process<T: Debug>(x: T) -> usize          fn process::<i32>(x: i32) -> usize        4-byte arg, no drop
  {                                            fn process::<String>(x: String) -> usize  24-byte arg, frees heap
      let text = format!("{x:?}");      ──►    fn process::<MyType>(x: MyType) -> usize  32-byte arg, frees Vec
      ...                                      fn process::<u8>(x: u8) -> usize          1-byte arg, no drop
  }
  type-checked ONCE, with T abstract:          each is an ordinary non-generic function: its own symbol,
  only what `T: Debug` promises may be used    its own machine code, optimized for its own T
```

Keep four levels apart. They answer different questions:

| Level | What exists | Question it answers |
|---|---|---|
| **Language** [LANG] | One definition with a contract (`T: Debug`) | Which calls are legal? |
| **Type checker** [RUSTC] | One generic body, checked against the bounds | Where are errors reported? |
| **Monomorphization** [RUSTC] | One *instance* per distinct set of type arguments | How many copies exist? |
| **Machine code** [CPU] | One symbol per instance, each optimized separately | What does each copy cost and how fast is it? |

Three consequences follow, and the rest of the Part expands on them. **Calls are static.** Each instance calls the exact
`Debug::fmt` for its `T`, so the optimizer can inline it. **Values are stored unboxed.** A `T` is stored in place, at
its real size, with no header and no pointer. **Code is duplicated.** Four types produce four copies, and Chapter 7.3 is
about keeping that under control.

### 3. Rust code

**The conceptual transformation, observed.** Here is `process<T>` called with four types (verified):

```rust
use std::any::type_name;
use std::fmt::Debug;
use std::mem::size_of;

#[derive(Debug)]
#[allow(dead_code)]
struct MyType {
    id: u32,
    tags: Vec<&'static str>,
}

/// One generic definition. The compiler generates one copy per concrete T it is used with.
fn process<T: Debug>(x: T) -> usize {
    let text = format!("{x:?}");
    println!(
        "process::<{}>  size_of::<T>() = {:>2}  value = {text}",
        type_name::<T>(),
        size_of::<T>()
    );
    text.len()
} // `x` is dropped here: for String and MyType that frees heap memory; for i32 it is nothing

fn main() {
    let total = process(42_i32) // T = i32, inferred from the argument
        + process(String::from("hello")) // T = String
        + process(MyType { id: 7, tags: vec!["vip"] }) // T = MyType
        + process::<u8>(255); // T = u8, written explicitly with the "turbofish"
    println!("total debug length = {total}");
}
```

```text
process::<i32>  size_of::<T>() =  4  value = 42
process::<alloc::string::String>  size_of::<T>() = 24  value = "hello"
process::<playground::MyType>  size_of::<T>() = 32  value = MyType { id: 7, tags: ["vip"] }
process::<u8>  size_of::<T>() =  1  value = 255
total debug length = 43
```

`type_name::<T>()` and `size_of::<T>()` are compile-time facts about each copy. The `i32` copy moves a 4-byte argument,
and the `MyType` copy moves 32 bytes and ends by freeing a `Vec`. Written out by hand, the compiler's output would look
like this sketch (not real syntax, since you can't define `process::<i32>` yourself):

```rust,ignore
// Conceptual expansion — what monomorphization produces, written as if by hand.
fn process_i32(x: i32) -> usize         { let text = format!("{x:?}"); /* ... */ text.len() }
fn process_String(x: String) -> usize   { let text = format!("{x:?}"); /* ... */ text.len() /* drop(x): free */ }
fn process_MyType(x: MyType) -> usize   { let text = format!("{x:?}"); /* ... */ text.len() /* drop(x): free Vec */ }
fn process_u8(x: u8) -> usize           { let text = format!("{x:?}"); /* ... */ text.len() }
```

**Bounds are checked at the definition.** The body of a generic function may use only what its bounds promise. Leave
out `PartialOrd`, and the error points at the *body*, before any caller exists (verified):

```rust,compile_fail
// The body uses `>`, but the signature never promised that T supports it.
fn largest<T: Copy>(xs: &[T]) -> Option<T> {
    let mut best = *xs.first()?;
    for &x in xs {
        if x > best {
            best = x;
        }
    }
    Some(best)
}
```

```text
error[E0369]: binary operation `>` cannot be applied to type `T`
 --> src/main.rs:6:14
  |
6 |         if x > best {
  |            - ^ ---- T
  |            |
  |            T
  |
help: consider further restricting type parameter `T` with trait `PartialOrd`
  |
3 | fn largest<T: Copy + std::cmp::PartialOrd>(xs: &[T]) -> Option<T> {
  |                    ++++++++++++++++++++++
```

With the bound in place, a caller whose type doesn't satisfy it gets the error at the **call site**, stated in terms of
the contract (verified, trimmed):

```text
error[E0277]: can't compare `Money` with `Money`
  --> src/main.rs:19:30
   |
19 |     println!("{:?}", largest(&payments)); // Money never said it can be compared
   |                      ------- ^^^^^^^^^ no implementation for `Money < Money` and `Money > Money`
   |                      |
   |                      required by a bound introduced by this call
   |
   = help: the trait `PartialOrd` is not implemented for `Money`
note: required by a bound in `largest`
```

The two errors split the blame precisely. E0369 says *the author promised too little*. E0277 says *the caller supplied a
type that doesn't keep the promise*. Neither error mentions the body's internals from the caller's side. That is the
property C++ templates lack (§8).

**Telling the compiler which `T`.** Type arguments are usually inferred from the arguments. When `T` appears only in the
*return* type, as with `str::parse::<F>()`, nothing constrains it (verified):

```text
error[E0284]: type annotations needed
 --> src/main.rs:3:9
  |
3 |     let port = "8080".parse().unwrap(); // parse::<F>() is generic over its RETURN type: F is unknown
  |         ^^^^          ----- type must be known at this point
  |
  = note: cannot satisfy `<_ as FromStr>::Err == _`
```

There are three fixes: annotate the binding, use the turbofish (`::<>`) on the call, or let a later use constrain the
type. A `where` clause keeps long bound lists readable, and it can constrain *associated types* of the parameters (here,
that each parse error can be displayed). Verified:

```rust
use std::collections::BTreeMap;
use std::fmt::Display;
use std::str::FromStr;

/// Parses "key=value" pairs into any key/value types that know how to parse themselves.
/// The `where` clause keeps a long list of bounds readable.
fn parse_pairs<K, V>(input: &str) -> Result<BTreeMap<K, V>, String>
where
    K: FromStr + Ord,
    V: FromStr,
    K::Err: Display,
    V::Err: Display,
{
    let mut out = BTreeMap::new();
    for pair in input.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').ok_or_else(|| format!("missing '=' in {pair:?}"))?;
        let key = k.trim().parse::<K>().map_err(|e| format!("bad key {k:?}: {e}"))?;
        let value = v.trim().parse::<V>().map_err(|e| format!("bad value {v:?}: {e}"))?;
        out.insert(key, value);
    }
    Ok(out)
}

fn main() {
    // Three ways to tell the compiler which types to instantiate:
    let port: u16 = "8080".parse().unwrap(); // 1. annotate the binding
    let retries = "3".parse::<u8>().unwrap(); // 2. turbofish on the call
    let limits = parse_pairs::<String, u32>("gold=1000, free=10").unwrap(); // 3. turbofish on a generic fn
    println!("port={port} retries={retries} limits={limits:?}");

    let weights: BTreeMap<u8, f64> = parse_pairs("1=0.5, 2=0.25").unwrap(); // inferred from the binding
    println!("weights={weights:?}");
    println!("{:?}", parse_pairs::<u8, u8>("1=300"));
}
```

```text
port=8080 retries=3 limits={"free": 10, "gold": 1000}
weights={1: 0.5, 2: 0.25}
Err("bad value \"300\": number too large to fit in target type")
```

That `main` creates three instances of `parse_pairs`: `<String, u32>`, `<u8, f64>`, and `<u8, u8>`. Each one gets a
parser specialized for its value type.

**Const generics: values as parameters.** [LANG] A type can be parameterized by a constant, most usefully an array
length. `[u8; 4]` and `[u8; 4096]` are different types, so each gets its own instance. Verified:

```rust
use std::mem::size_of;

/// FNV-1a over a fixed-size block: N is part of the TYPE, so each block size gets its own code.
fn checksum<const N: usize>(block: &[u8; N]) -> u32 {
    block.iter().fold(0x811c_9dc5u32, |h, &b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193))
}

/// A fixed-capacity ring buffer: no heap allocation, capacity known at compile time.
struct Ring<T: Copy + Default, const N: usize> {
    items: [T; N],
    head: usize,
    len: usize,
}

impl<T: Copy + Default, const N: usize> Ring<T, N> {
    fn new() -> Self {
        Ring { items: [T::default(); N], head: 0, len: 0 }
    }

    /// Pushes a value, overwriting the oldest one when full.
    fn push(&mut self, value: T) {
        let tail = (self.head + self.len) % N;
        self.items[tail] = value;
        if self.len < N {
            self.len += 1;
        } else {
            self.head = (self.head + 1) % N;
        }
    }

    fn iter(&self) -> impl Iterator<Item = T> + '_ {
        (0..self.len).map(move |i| self.items[(self.head + i) % N])
    }
}

fn main() {
    let header = [0xCA, 0xFE, 1, 0];
    let page = [7u8; 4096];
    println!("checksum::<4>    = {:#010x}", checksum(&header));
    println!("checksum::<4096> = {:#010x}", checksum(&page));

    let mut last_latencies: Ring<u32, 4> = Ring::new();
    for ms in [12, 40, 7, 95, 3, 61] {
        last_latencies.push(ms);
    }
    let kept: Vec<u32> = last_latencies.iter().collect();
    println!("last 4 latencies: {kept:?}");
    println!("size_of::<Ring<u32, 4>>()  = {} bytes (no heap)", size_of::<Ring<u32, 4>>());
    println!("size_of::<Ring<u64, 64>>() = {} bytes (no heap)", size_of::<Ring<u64, 64>>());
}
```

```text
checksum::<4>    = 0xe5da0598
checksum::<4096> = 0xe0616dc5
last 4 latencies: [7, 95, 3, 61]
size_of::<Ring<u32, 4>>()  = 32 bytes (no heap)
size_of::<Ring<u64, 64>>() = 528 bytes (no heap)
```

`% N` in `Ring::<u32, 4>` divides by the constant 4, which the optimizer can turn into a bit mask. [VERSION] Stable Rust
(since 1.51) allows const parameters of integer, `bool`, and `char` types, used as whole values. Computing *with* them in
types, as in `[T; N + 1]`, needs the unstable `generic_const_exprs` feature.

**`impl Trait` in argument position.** `fn f(x: impl Trait)` is shorthand for `fn f<T: Trait>(x: T)` with an anonymous
`T`. Projects L1 and L2 used it for their I/O. `ingest(mut input: impl BufRead, ...)` in `logstat` and `process(mut
input: impl BufRead, output: &mut impl Write, ...)` in `redact` are generic functions, and each distinct reader type is a
separate instance. The shape, verified:

```rust
use std::io::{self, BufRead, BufReader, Cursor, Write};

/// Projects L1 and L2 used this shape: `impl BufRead` in, `impl Write` out.
/// Each distinct (reader, writer) pair a program uses becomes its own copy of this function.
fn number_lines(input: impl BufRead, out: &mut impl Write) -> io::Result<usize> {
    let mut n = 0;
    for line in input.lines() {
        n += 1;
        writeln!(out, "{n:>4} | {}", line?)?;
    }
    Ok(n)
}

fn main() -> io::Result<()> {
    // Instantiation 1: an in-memory byte slice, as in a unit test.
    let mut buf: Vec<u8> = Vec::new();
    number_lines(&b"alpha\nbeta\n"[..], &mut buf)?;
    print!("{}", String::from_utf8_lossy(&buf));

    // Instantiation 2: a buffered reader over something that implements Read, writing to stdout.
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    let n = number_lines(BufReader::new(Cursor::new("gamma\ndelta\nepsilon\n")), &mut lock)?;
    writeln!(lock, "{n} lines")?;
    Ok(())
}
```

```text
   1 | alpha
   2 | beta
   1 | gamma
   2 | delta
   3 | epsilon
3 lines
```

You can see the instances in the real `logstat` binary. Its debug assembly (Project L1's listing, emitted as a binary
crate) contains exactly these generic symbols from the project's own code:

```text
playground::ingest::<std::io::buffered::bufreader::BufReader<std::fs::File>>:
playground::ingest::<std::io::stdio::StdinLock>:
playground::report::write::<std::io::buffered::bufwriter::BufWriter<std::io::stdio::StdoutLock>>:
```

Files get one copy and stdin gets another. The `&[u8]` instance used by the unit tests exists only in the test build.
Return-position `impl Trait`, which is a different feature despite the shared syntax, is in Chapter 7.3.

---

## Pass 2 · Systems level — *From generic MIR to per-type machine code*

### 4. Under the hood

**Step 1: type-check once.** [RUSTC] When rustc checks `largest<T: PartialOrd + Copy>`, `T` is a *placeholder* type: it
supports exactly the operations its bounds provide (Chapter 18.3, *Type Checking and Trait Solving*, covers the trait
solver). `x > best` resolves to `<T as PartialOrd>::gt`, a call whose implementation isn't known yet. The body's MIR is
built and borrow-checked once, still generic.

**Step 2: collect instances.** [RUSTC] After type checking, the **monomorphization collector** walks the program from
its roots: `main`, or in a library the public non-generic functions and statics. Each time it finds a call to a generic
function with concrete type arguments, it records a *mono item* such as `largest::<u8>`. Then it walks *that* instance's
body with `T = u8` substituted and finds more calls: `<u8 as PartialOrd>::gt`, and so on. The walk continues until no
new items appear. The resulting set is **everything that will become machine code**. A generic function that is never
called with concrete types produces no code at all. The collector then splits the items into *codegen units* (CGUs), the
chunks that LLVM compiles in parallel (Chapter 18.6).

**Step 3: generate code per instance.** Each mono item becomes an ordinary LLVM function with an ordinary symbol. Here
is the verified release LLVM IR for `largest` and `byte_len` from `listings/part-07/ch01-02-instantiations.rs`, where
non-generic `pub fn`s call `largest` with `u8`, `i64`, and `f64`, and call `byte_len<T: AsRef<[u8]>>` with an owned
`Vec<u8>` and a borrowed `&[u8]` (trimmed to the comment and symbol of each function):

```text
; playground::largest::<f64>
define { i64, double } @_RINvCsjau7DNBNlby_10playground7largestdEB2_(...)
; playground::largest::<u8>
define { i1, i8 }      @_RINvCsjau7DNBNlby_10playground7largesthEB2_(...)
; playground::largest::<i64>
define { i64, i64 }    @_RINvCsjau7DNBNlby_10playground7largestxEB2_(...)
; playground::byte_len::<alloc::vec::Vec<u8>>
define noundef i64     @_RINvCsjau7DNBNlby_10playground8byte_lenINtNtCs6i54tJFfzR_5alloc3vec3VechEEB2_(...)
; playground::byte_len::<&[u8]>
define noundef ... i64 @_RINvCsjau7DNBNlby_10playground8byte_lenRShEB2_(...)
```

These are **v0 mangled names**, and they spell out the type arguments. `_R` marks a v0 symbol. `I … E` wraps a generic
instantiation. `Nv…7largest` is the path `playground::largest`. Between them sit the type arguments: `d` is `f64`, `h`
is `u8`, `x` is `i64`, `RSh` is `&[u8]` (reference, slice, `u8`), and `INtNt…5alloc3vec3VechE` is `alloc::vec::Vec<u8>`.
The trailing `B2_` is a back-reference naming the crate that instantiated the function. [VERSION] rustc 1.98.1 on the
Playground emitted v0 symbols by default (observed above). Older toolchains defaulted to the "legacy" scheme
(`_ZN…17h<hash>E`), where type arguments survived only as a hash. The flag `-C symbol-mangling-version=v0` has been
stable since 1.59. The practical payoff is that profilers, debuggers, and crash reports that demangle v0 show
`largest::<u8>` and `largest::<f64>` as separate, readable frames.

Look at the return types, too. `Option<u8>` comes back as `{ i1, i8 }` (a flag and a byte in two registers), while
`Option<f64>` is `{ i64, double }`. Each copy has its own ABI, derived from its own `T`.

**Each copy is optimized for its type.** Here is the verified release assembly for the inner loops (trimmed):

```text
playground::largest::<u8>:                    playground::largest::<i64>:
  movzx  r9d, byte ptr [rax]                    mov    r10, qword ptr [rcx]
  cmp    r9b, cl                                cmp    r10, r8
  cmova  ecx, r9d        ; unsigned "above"     cmovg  r8, r10         ; signed "greater"
  ...unrolled 4x...                             ...unrolled 4x...

playground::largest::<f64>:
  movsd   xmm2, qword ptr [rcx]
  maxsd   xmm4, xmm1
  cmpltsd xmm1, xmm2     ; float compare into a mask, then and/andn/or to select
```

That is one source line, `if x > best`, compiled three ways: an unsigned conditional move, a signed conditional move,
and SSE floating-point compare-and-blend that follows IEEE semantics for `>`. None of them is a function call. The
`PartialOrd::gt` call from step 1 was resolved to the concrete implementation and inlined *because* the type was known.

**Drop glue is per type as well.** The same `byte_len` source compiles to two very different functions (verified
release asm):

```text
playground::byte_len::<alloc::vec::Vec<u8>>:       playground::byte_len::<&[u8]>:
  push  rbx                                          mov  rax, rsi     ; the length is the 2nd register
  mov   rsi, qword ptr [rdi]      ; capacity         ret
  mov   rbx, qword ptr [rdi + 16] ; length
  test  rsi, rsi
  je    .LBB3_2
  mov   rdi, qword ptr [rdi + 8]  ; pointer
  mov   edx, 1
  call  qword ptr [rip + __rustc::__rust_dealloc@GOTPCREL]
.LBB3_2:
  mov   rax, rbx
  pop   rbx
  ret
```

The owning instance takes ownership of the `Vec`, so it must free the buffer on the way out (Chapter 3.1's drop, now per
instance). The borrowing instance is just "return the length". The wrappers that call them, such as `largest_u8`,
compiled to a single `jmp` into the instance: a tail call.

**Lifetimes are not monomorphized.** [RUSTC] Chapter 4.3 promised this. Type parameters produce copies, and lifetime
parameters don't. `fn longest<'a>(a: &'a str, b: &'a str) -> &'a str` is compiled once however many regions its callers
use, because lifetimes are erased after borrow checking. They affect whether a program is accepted, never the code
generated for it. In a mono item such as `ingest::<StdinLock>`, the `StdinLock<'_>` lifetime is gone, as the symbol above
shows.

**Across crates: the recipe ships, your crate cooks.** [RUSTC] Chapter 2.1 showed that an `.rlib` carries the MIR of its
generic functions. When your crate calls `serde_json::to_string::<Order>`, the instance is created **in your crate's**
codegen, because the library could not know about `Order`. This is why generic-heavy dependencies make *your* build slow
(Chapter 7.3). [RUSTC] In unoptimized builds, rustc by default lets a crate reuse an instance that an upstream crate has
already generated and exported ("shared generics"), which avoids some duplicate work. Optimized builds generate their own
copies so that they can inline them.

**Explaining Chapter 1.3.** Chapter 1.3 showed that `data.iter().filter(..).map(..).sum()` and a hand-written index loop
compile to the same assembly. It promised the explanation would come here. Every adapter in that chain is a generic
struct, and each closure is a distinct anonymous type. So the chain is a tower of instances, each specialized for the
exact closure types it wraps. Emit the debug LLVM IR for those two functions (the listing is
`ch01-08-iter-instantiations.rs`) and count the functions: **22** (`define` lines). Here are the ones that come from the
iterator chain, abridged:

```text
; playground::sum_even_squares_iter
; playground::sum_even_squares_iter::{closure#0}
; playground::sum_even_squares_iter::{closure#1}
; <core::slice::iter::Iter<u64> as core::iter::traits::iterator::Iterator>::filter::<playground::sum_even_squares_iter::{closure#0}>
; <core::iter::adapters::filter::Filter<core::slice::iter::Iter<u64>, playground::sum_even_squares_iter::{closure#0}> as core::iter::traits::iterator::Iterator>::map::<u64, playground::sum_even_squares_iter::{closure#1}>
; <core::iter::adapters::map::Map<core::iter::adapters::filter::Filter<...>, ...> as core::iter::traits::iterator::Iterator>::sum::<u64>
; <u64 as core::iter::traits::accum::Sum>::sum::<core::iter::adapters::map::Map<...>>
; <core::slice::iter::Iter<u64> as core::iter::traits::iterator::Iterator>::fold::<u64, core::iter::adapters::filter::filter_fold<...>::{closure#0}>
; core::iter::adapters::filter::filter_fold::<&u64, u64, playground::sum_even_squares_iter::{closure#0}, ...>::{closure#0}
; core::iter::adapters::map::map_fold::<&u64, u64, u64, playground::sum_even_squares_iter::{closure#1}, ...>::{closure#0}
; ... 12 more (the loop version's Range<usize> iteration, slice helpers, debug-assertion checks)
```

In the debug build, `sum_even_squares_iter` really does call `filter`, then `map`, then `sum`, which calls `fold`, which
calls the per-closure `filter_fold` and `map_fold` closures. Emit the **release** IR for the same file, and there are **2**
functions: `sum_even_squares_iter` and `sum_even_squares_loop`. Because every call in the tower was *static* (the
instance was known) and small, LLVM inlined all of them into their callers, until only the loop remained. Then the
ordinary loop optimizations (bounds-check elimination, unrolling, the branchless `cmov`) produced Chapter 1.3's assembly.

**Monomorphization makes the calls static, and inlining removes them.** Neither step alone would do it. A single shared
copy of `filter` for all closures would need an indirect call per element. That is what `dyn Fn` does, and what a JVM
must undo through profiling (Chapter 7.2). Part X, *Closures, Iterators, and Zero-Cost Abstractions*, takes the chain
apart adapter by adapter.

**The exception to "checked at the definition".** [LANG] [RUSTC] A few things can only be evaluated once `T` or `N` is
known, and those checks happen *during monomorphization*. The main case is constant evaluation that depends on generic
parameters (verified):

```rust,compile_fail
/// The one place a generic body is checked PER INSTANTIATION: constant evaluation.
fn first_byte<const N: usize>(block: [u8; N]) -> u8 {
    const { assert!(N > 0, "first_byte needs a non-empty block") };
    block[0]
}

fn main() {
    println!("{}", first_byte([7, 8, 9])); // N = 3: the assertion holds
    println!("{}", first_byte([])); // N = 0: fails while instantiating first_byte::<0>
}
```

```text
error[E0080]: evaluation panicked: first_byte needs a non-empty block
 --> src/main.rs:4:13
  |
4 |     const { assert!(N > 0, "first_byte needs a non-empty block") };
  |             ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ evaluation of `first_byte::<0>::{constant#1}` failed here
...
note: the above error was encountered while instantiating `fn first_byte::<0>`
```

"While instantiating" is the tell. This is a **post-monomorphization error**, and it is useful: it turns an invalid
constant into a build failure instead of a run-time panic. It has one operational quirk. [RUSTC] `cargo check` stops
before monomorphization, so it can miss errors like this one that `cargo build` reports. Keep a full build in CI.

### 5. Memory

**Values: unboxed, at their real size.** [LANG] A `T` is stored exactly like a value of the concrete type would be.
`process::<u8>` takes a 1-byte argument, and `process::<MyType>` a 32-byte one. A `Vec<T>` stores `T`s contiguously, and
`Ring<u32, 4>` is 32 bytes with its storage inline: four `u32`s (16 bytes) plus `head` and `len` (8 bytes each). A
`Ring<u64, 64>` is 528 bytes. There is no header, no pointer, and no heap allocation. Generic code never boxes on your
behalf. This is the single biggest difference from Java (Chapter 7.2 measures it).

**Code: one copy per instance, in the binary's text section.** Every instance occupies space in `.text` and, when it
runs, in the instruction cache. `largest::<u8>`, `<i64>`, and `<f64>` are three separate blocks of code with separate
addresses. In debug builds, instances also carry their own debug info, which is a large part of why debug binaries of
generic-heavy programs are big. The cost is linear in *distinct instantiations actually used*, not in call sites. A
thousand calls to `largest::<u8>` share one copy. Chapter 7.3 measures how instances multiply, and how to stop them.

**Stack frames per instance.** Each instance has its own frame layout, sized for its `T`. A generic recursive function
over a large `T` uses a large frame in *that* instance only.

### 6. CPU / OS

**Static calls are cheap, and they enable inlining.** [CPU] A direct `call` to a known address is predicted perfectly
by the CPU's front end. Better still, as the release IR showed, the call usually disappears: the compiler inlines the
concrete `gt` or `fmt` into the instance and then optimizes across the old boundary (constant folding, vectorization,
`cmov`). Inlining is what turns static dispatch into speed. Chapter 6.5, *Static vs Dynamic Dispatch: The Architect's
Decision*, shows the indirect-call alternative, and Chapter 7.2 shows it next to the static version in assembly.

**Instruction-cache pressure is the counterweight.** [CPU] Ten instances of a hot 2 KB function occupy 20 KB of code.
If they all run in the same request path, they compete for a 32 KB L1 instruction cache (a typical size on current
x86-64 cores; check your part). This rarely matters, but when it does it shows up in profiles as front-end stalls rather
than as any single hot function. Profile before de-genericizing for this reason.

**The OS and your tools see instances as ordinary functions.** [OS] Symbols such as
`playground::largest::<u8>` appear in `perf report`, flame graphs, `gdb` backtraces, and panic backtraces, each
demangled with its type arguments. A flame graph therefore separates `parse_pairs::<String, u32>` from
`parse_pairs::<u8, f64>`, which is a real diagnostic advantage over a JVM profile, where one shared method body shows up
as one frame whatever the types were.

---

## Pass 3 · Architect level — *Choosing the shape of a generic API*

### 7. Trade-offs

| Form | Copies of the code | Dispatch | Caller can name the type? | Main cost | Choose when |
|---|---|---|---|---|---|
| `fn f<T: Tr>(x: T)` | One per `T` used | Static, inlinable | Yes (turbofish) | Binary size, compile time | Hot paths; the caller's type matters |
| `fn f(x: impl Tr)` | One per `T` used | Static, inlinable | No | Same as above | Same, when no caller needs to name `T` |
| `fn f(x: &dyn Tr)` | One | Indirect via vtable | n/a | An indirect call per method; no inlining across it | Cold paths, plugins, heterogeneous collections |
| `fn f(x: Concrete)` | One | Static | n/a | Callers must convert | Internal APIs with one real type |
| `fn f(x: MyEnum)` | One | `match` (a jump table or compares) | n/a | Closed set of types | A small, closed set of variants |
| `const N: usize` | One per `N` used | Static, `N` folded in | Yes | One copy per size | Sizes known at compile time (buffers, windows) |

Two rules of thumb. **Make it generic where the type changes the machine code**, as with numbers, element types in hot
loops, and closures. **Make it concrete or `dyn` where it doesn't**, as with paths, error messages, logging sinks, and
configuration. Chapter 7.3 turns this into a technique.

### 8. Java comparison

**Definition checking is familiar ground.** A Java engineer already has it: `<T extends Comparable<T>> T max(List<T>)`
is checked at its definition, and a body that calls a method not on `Comparable` is rejected. The Java model differs in
what happens *after* checking. There is one compiled body, `T` becomes `Comparable` (its erasure), and `compareTo` is an
interface call on whatever object arrives. Chapter 7.2 is the deep comparison. The short version for this chapter:

| Question | Java generics | Rust generics |
|---|---|---|
| Checked where? | At the definition | At the definition (plus post-mono const evaluation) |
| Copies of the code | One | One per instantiation |
| `int`/`long` as `T`? | No: boxed to `Integer`/`Long` | Yes: `u32`, `i64`, `f64` directly |
| Call to a bound's method | Interface call, devirtualized by the JIT *if* the profile allows | Static call, usually inlined |
| `T` known at run time? | No (erased) | Yes, per copy (`size_of::<T>()`, `type_name::<T>()`) |
| Array-length parameters | No | `const N: usize` |

**C++ is the other half of the contrast.** A C++ function template is type-checked when it is instantiated. A call to
`std::sort` with a type that lacks `operator<` produces an error from *inside* the library's implementation, often many
screens long, pointing at code the caller never wrote. C++20 concepts improved the call-site message, but a
constrained template's body still isn't checked against its concept. A body can use an operation the concept never
promised, and the mistake surfaces only for a type that lacks it. Rust checks both sides of the contract: E0369 for the
author and E0277 for the caller.

> **Analogy limit.** "`T: Ord` is Rust's `T extends Comparable<T>`" holds at the type-checking level. It breaks at the
> machine level. The Java bound means "`T` is some object whose class implements `compareTo`, found at run time". The
> Rust bound means "for each concrete `T`, the compiler will find `T`'s `cmp` and call it directly". It also breaks for
> primitives: there is no Java `max` over `long[]` that uses the generic method without boxing every element.

### 9. Production scenario

**Meridian's gateway: per-route latency windows and readable profiles.** The gateway (Chapters 1.2 and 2.7) keeps, for
each of its ~120 upstreams, the most recent latency samples for adaptive timeouts. The Java version used an
`ArrayDeque<Long>` per upstream: a heap object per sample, boxed, and garbage per request. The Rust `gateway-core` crate
uses a const-generic ring like the one in §3, `Ring<u32, 64>`, embedded directly in each upstream's state. That is a
fixed 272 bytes per upstream by the same arithmetic as §5 (64 × 4 bytes of samples plus 16 bytes of indices), with no
allocation after startup. The window size is part of the type, so the one route class that needed a longer window uses
`Ring<u32, 256>`. That makes two instances and two small copies of the ring's code, with `% N` folded into a mask in
each.

The second payoff came during an incident review. A CPU flame graph of the gateway showed two separate frames for the
config parser, `parse_pairs::<String, u32>` (rate limits) and `parse_pairs::<u8, f64>` (weights), because v0 symbols
carry type arguments. The rate-limit instance was hot because a sidecar was re-pushing configuration every second. In
the JVM version, the equivalent parser had been one method, `parsePairs`, with one frame in the profile, and nobody had
connected it to the rate-limit reloads. The fix was in the sidecar. The generics simply made the evidence readable.

### 10. Failure scenario

**The generic that was lossy for one of its types.** Meridian's fraud feature extraction (Chapter 1.2) de-duplicates
IDs before scoring. A helper crate had a home-grown "numeric" trait so that one generic function could feed a
statistics library that wanted `f64`. It worked for months on `u32` merchant IDs. Then someone called it with `u64`
user IDs (verified):

```rust
use std::collections::HashSet;

/// A home-grown "numeric" trait: convenient, generic... and silently lossy for u64.
trait ToF64 {
    fn to_f64(self) -> f64;
}
impl ToF64 for u32 {
    fn to_f64(self) -> f64 {
        self as f64 // exact: every u32 fits in f64's 53-bit mantissa
    }
}
impl ToF64 for u64 {
    fn to_f64(self) -> f64 {
        self as f64 // NOT exact above 2^53
    }
}

/// Generic de-duplication keyed on the f64 representation (e.g. to feed a stats library).
fn distinct_count<T: ToF64 + Copy>(ids: &[T]) -> usize {
    ids.iter().map(|&id| id.to_f64().to_bits()).collect::<HashSet<u64>>().len()
}

fn main() {
    let small: [u32; 3] = [1, 2, 3];
    let user_ids: [u64; 3] = [9_007_199_254_740_992, 9_007_199_254_740_993, 9_007_199_254_740_994];
    println!("distinct u32 ids: {} of {}", distinct_count(&small), small.len());
    println!("distinct u64 ids: {} of {}", distinct_count(&user_ids), user_ids.len());
    println!("2^53 + 1 as f64 = {}", 9_007_199_254_740_993u64 as f64);
}
```

```text
distinct u32 ids: 3 of 3
distinct u64 ids: 2 of 3
2^53 + 1 as f64 = 9007199254740992
```

Three distinct users counted as two. At Meridian's scale, IDs above 2^53 were rare but real (a partner imported
snowflake-style IDs). Two users merged in the fraud features and got each other's velocity counts. This is the same
2^53 boundary as Chapter 2.3's money-in-JSON rule, reached through a different path.

The generic function itself was fine: it did exactly what its bound promised. **The bound promised the wrong thing.**
`ToF64` said "convertible to `f64`" without saying "losslessly". The standard library draws that line in its trait
impls: `From<u32> for f64` exists, and `From<u64> for f64` does not. A bound on `Into<f64>` would have rejected the call
at compile time (verified):

```text
error[E0277]: the trait bound `f64: From<u64>` is not satisfied
 --> src/main.rs:9:25
  |
9 |     println!("{}", mean(&[1u64, 2, 3])); // rejected: std has no From<u64> for f64, because it can lose precision
  |                    ---- ^^^^^^^^^^^^^ the trait `From<u64>` is not implemented for `f64`
  |                    |
  |                    required by a bound introduced by this call
  |
  = help: `f64` implements trait `From<T>`:
            From<bool>
            From<f16>
            From<f32>
            From<i16>
            From<i32>
            From<i8>
            From<u16>
            From<u32>
            From<u8>
```

The real fix was to de-duplicate on the IDs themselves, with `T: Hash + Eq`, and convert only the *counts* to `f64`.
The lesson for API design: **a trait bound is a semantic contract, and every instance inherits it.** Adding an `impl`
for a new type is adding a new instance to every generic function that uses the trait, so review impls as carefully as
functions.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part VII).*

1. Walk `fn process<T: Debug>(x: T)` from source to machine code for `T = i32` and `T = String`. What differs between
   the two instances, and why?
2. What is the monomorphization collector, what are its roots, and why does an unused generic function produce no code?
3. Why does Rust report a missing bound at the generic function's definition, and C++ at the template's instantiation?
   What do C++20 concepts change, and what don't they?
4. Name a check that Rust performs per instantiation rather than at the definition. What is the operational consequence
   for `cargo check`?
5. Read this symbol aloud: `_RINvCsjau7DNBNlby_10playground8byte_lenRShEB2_`. Why does it matter in production that
   type arguments appear in symbol names?
6. Chapter 1.3's iterator chain and index loop compile to the same assembly. Explain why, in terms of monomorphization
   and inlining, and say what each step contributes on its own.
7. Are lifetime parameters monomorphized? Why or why not?
8. When is a generic function instantiated in *your* crate rather than in the library that defines it, and what does
   that mean for build times?

### 12. Exercises

- **Beginner.** Add a fifth call to `process` with `T = &str` and `T = Vec<u8>`. Predict `size_of::<T>()` for each
  before running.
- **Intermediate.** Rewrite `parse_pairs` so that it returns `impl Iterator<Item = Result<(K, V), String>>` instead
  of collecting into a map. Which bounds move, and which instances does the new `main` create?
- **Advanced.** Give `Ring` a `const { assert!(N.is_power_of_two()) }` check in `new`, and replace `% N` with a mask.
  Show the post-monomorphization error for `Ring::<u32, 6>`. Then argue whether this belongs in the type (a const
  check) or in documentation.
- **Systems.** Emit release assembly for `largest` with `u16`, `i8`, and `f32` as well. Predict the conditional-move
  variant (`cmova`, `cmovg`, `cmovb`, …) or the SSE sequence each will use before looking.
- **Architecture.** List every generic function in a Rust service you know (or in Project L2). For each, name the
  instances the binary actually contains and state whether the genericity pays for itself.

### 13. Debugging exercise

A teammate writes a generic average for Meridian's metrics and calls it with request counts:

```rust,compile_fail
/// The std-only version: bound on `Into<f64>`, the LOSSLESS conversion trait.
fn mean<T: Copy + Into<f64>>(xs: &[T]) -> f64 {
    xs.iter().map(|&x| x.into()).sum::<f64>() / xs.len() as f64
}

fn main() {
    println!("{}", mean(&[1u32, 2, 3])); // fine: From<u32> for f64 exists
    println!("{}", mean(&[1u64, 2, 3])); // rejected: std has no From<u64> for f64, because it can lose precision
}
```

1. Predict the error code and which line it points at. Is the author or the caller "at fault" by the E0369/E0277 split?
2. Propose three fixes: one that changes the bound, one that changes the caller, and one that changes the algorithm.
   Which is right for request counts, and which for account IDs?
3. Why is the compile error better than the behavior of the `ToF64` version in §10?

### 14. Design exercise

**Meridian's metrics facade.** Teams want one API to record counters, gauges, and histograms from any numeric type:
`record(name, value)`. Design three versions: a generic `record<T: ...>(name: &str, value: T)`, a concrete
`record(name: &str, value: f64)` with conversions at the call site, and a `record(name: &str, value: Metric)` taking an
enum. For each, state the bound (if any) and what it rules out, the number of instances in a service with 400 call sites
using six numeric types, what happens with `u64` values above 2^53, and how each appears in a flame graph. Recommend
one, and name the measurement that would change your mind.
