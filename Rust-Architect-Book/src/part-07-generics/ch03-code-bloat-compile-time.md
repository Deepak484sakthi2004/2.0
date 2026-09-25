# Chapter 7.3 — Code Bloat, Compile Time, and How to Control Them

> **Where this sits:** Part VII · Generics and Monomorphization · chapter 3 of 3
> **Prerequisites:** Chapters 7.1 and 7.2. Chapter 1.4's compile-time anatomy, and Chapter 2.1 (metadata, `cargo check`).
> **After this chapter you can:** predict how a generic function's cost grows with its instantiations and measure it
> from LLVM IR; apply the non-generic inner function pattern and prove that it worked; choose `dyn` at cold boundaries;
> use `cargo check`, crate splits, and build timings deliberately; design with return-position `impl Trait`, including
> edition 2024's capture rules and `use<..>`; and set a code-size budget a CI job can enforce.

---

## Pass 1 · User level — *Paying for monomorphization, and paying less*

### 1. Problem

Chapter 7.1 showed that each instantiation is a separate function, and Chapter 7.2 showed what that buys at run time.
This chapter is about the bill. Every instance is type-substituted, lowered to LLVM IR, optimized, and turned into
machine code, and every instance lands in the binary. The costs show up in three places:

- **Compile time.** Chapter 1.4 listed monomorphization among the main reasons Rust builds are slow and promised
  mitigations. Here they are, each backed by a measurement where the Playground allows one.
- **Binary size.** This matters for containers pulled onto thousands of nodes, for Lambda cold starts, for embedded
  targets, and for the instruction cache.
- **Your edit loop.** A change to a widely used generic function recompiles its instances in every crate that uses
  them.

The chapter also finishes the `impl Trait` story. Return-position `impl Trait` hides a concrete type behind a trait.
Edition 2024 changed which lifetimes that hidden type is assumed to hold, which affects both API design and edition
migrations.

### 2. Mental model

**The cost of a generic function is its body multiplied by its instantiations, and closures multiply with it.**

```text
 cost(generic fn) ≈ size(body, including everything it pulls in) × distinct instantiations

 FAT generic                               THIN generic shell + non-generic core
 fn load<P: AsRef<Path>>(p: P) {           fn load<P: AsRef<Path>>(p: P) {
     ...40 lines using p...                    fn inner(p: &Path) { ...40 lines... }   compiled ONCE
 }                                             inner(p.as_ref())                         shell: 1 line per P
                                           }
 &str, String, &Path, PathBuf              &str, String, &Path, PathBuf
   → 4 × (body + its closures + the          → 4 × (one-line shell) + 1 × (body + closures + ...)
      generic std code they instantiate)
```

There are four levers. Each has its own run-time price, and §7 compares them:

1. **Measure** which generic functions cost the most (IR lines per function, summed over instances).
2. **Shrink each instance**: keep only the type-dependent part generic (the inner function pattern).
3. **Reduce the number of instances**: take concrete types or `&dyn Trait` where the type doesn't change the machine
   code.
4. **Move the work**: `cargo check` in the edit loop, crate splits for parallelism and caching, and faster back ends
   for debug builds.

### 3. Rust code

**How instances multiply, measured.** A reporting helper that sorts, de-duplicates, and formats is the kind of function
every service writes once and calls with every row type:

```rust,ignore
use std::fmt::Debug;

/// A reporting helper written once and called with every row type in the service.
pub fn top_n<T: Ord + Clone + Debug>(rows: &[T], n: usize) -> Vec<String> {
    let mut sorted = rows.to_vec();
    sorted.sort();
    sorted.dedup();
    sorted.iter().rev().take(n).map(|r| format!("{r:?}")).collect()
}

macro_rules! row_types {
    ($($name:ident),* $(,)?) => {
        $(
            #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
            pub struct $name(pub u32, pub String);
        )*
        /// Calls top_n once per row type, forcing one instantiation each.
        pub fn report_all(n: usize) -> usize {
            let mut lines = 0;
            $( lines += top_n(&[$name(3, "c".into()), $name(1, "a".into())], n).len(); )*
            lines
        }
    };
}

row_types!(R0, R1, R2, R3,);
```

Three listings (`ch03-02-growth-1.rs`, `ch03-03-growth-4.rs`, `ch03-04-growth-16.rs`) differ only in how many row
types the macro generates. I emitted LLVM IR and assembly for each and counted the output: functions are `define`
lines, and assembly is instruction lines in the Playground's filtered output. All figures are verified on rustc 1.98.1:

| Row types | Debug IR functions | Debug IR lines | Release IR functions | Release IR lines | Release asm instructions |
|---|---|---|---|---|---|
| 1 | 171 | 22,437 | 13 | 3,628 | 2,116 |
| 4 | 483 | 68,262 | 25 | 11,320 | 6,919 |
| 16 | 1,731 | 251,562 | 73 | 42,088 | 26,082 |
| **Each extra type** | **+104** | **+15,275** | **+4** | **+2,564** | **≈ +1,600** |

The growth is **exactly linear**: each row type adds the same amount. The per-type cost is not `top_n`'s five lines. It
is everything `top_n` *instantiates*: `to_vec` and `clone` for `T`, the standard library's stable sort for `T` (several
functions, since the sort is itself generic code), `dedup`, the `Debug` formatting of `&T`, the iterator adapters over
`T`, and drop glue for `Vec<T>`. In the debug build, all 104 functions per type are separate. In the release build,
most are inlined, and 4 per type remain out of line: the sort's `quicksort`, `drift::sort`, and `driftsort_main`, plus
`<&R as Debug>::fmt`.

The release IR holds one more observation. [RUSTC] The small sort helpers (`sort4_stable`, `insertion_sort_shift_left`,
`median3_rec`) appear only **once**, as the `R0` instance, and all 16 `quicksort` instances call `sort4_stable::<R0, …>`.
The row types have identical layouts and identical derived comparisons, so LLVM merged the byte-identical instances into
one (rustc enables LLVM's function-merging pass in optimized builds). The large functions were *not* merged, and types
with different layouts can never be merged. Treat merging as a bonus, not a strategy.

**The non-generic inner function.** This is the pattern Chapter 1.4 named, and the standard library uses it throughout
(`std::fs::read` has this shape). Two versions of the same config loader, one generic all the way through and one with a
thin shell (library listing `ch03-01-inner-fn.rs`):

```rust,ignore
use std::path::{Path, PathBuf};

/// A stand-in for real work: parse "key = value" lines, validate, and summarize.
fn parse_body(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (k, v) = line.split_once('=').ok_or_else(|| format!("line {}: expected key = value", n + 1))?;
        out.push((k.trim().to_string(), v.trim().to_string()));
    }
    Ok(out)
}

pub mod fat {
    use super::*;
    /// The whole body is generic over P, so it is duplicated for every P a caller uses.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<usize, String> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let pairs = parse_body(&text)?;
        let mut keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
        keys.sort_unstable();
        keys.dedup();
        if keys.len() != pairs.len() {
            return Err(format!("{}: duplicate keys", path.display()));
        }
        Ok(pairs.len())
    }
}

pub mod thin {
    use super::*;
    /// Only the one-line conversion is generic; the body is compiled exactly once.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<usize, String> {
        fn inner(path: &Path) -> Result<usize, String> {
            let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let pairs = parse_body(&text)?;
            let mut keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
            keys.sort_unstable();
            keys.dedup();
            if keys.len() != pairs.len() {
                return Err(format!("{}: duplicate keys", path.display()));
            }
            Ok(pairs.len())
        }
        inner(path.as_ref())
    }
}

/// Four callers, four path types: &str, String, &Path, PathBuf.
pub fn callers() -> [Result<usize, String>; 8] {
    [
        fat::load("a.conf"),
        fat::load(String::from("b.conf")),
        fat::load(Path::new("c.conf")),
        fat::load(PathBuf::from("d.conf")),
        thin::load("a.conf"),
        thin::load(String::from("b.conf")),
        thin::load(Path::new("c.conf")),
        thin::load(PathBuf::from("d.conf")),
    ]
}
```

Here is what the debug LLVM IR contains, counted per function and grouped by which `load` each function belongs to
(verified):

| | IR functions | IR lines | Per additional path type |
|---|---|---|---|
| `fat::load` and everything instantiated from it | 88 | 4,830 | +22 functions, ~1,200 lines |
| `thin::load` shells + `thin::load::inner` and everything instantiated from it | 26 | 1,345 | +1 function, ~45 lines |

Why 22 functions per path type rather than 1? Because **closures inside a generic function are generic too.** The
`map_err` closure in `fat::load::<&str>` is a different type from the one in `fat::load::<String>`, even though neither
closure uses `P`. So each one drags its own `Result::map_err::<…>` instance, its own `Map<…>` iterator instances, its own
`extend_trusted`, `fold`, and drop glue. In the IR they appear with the path type embedded in their names:

```text
; <core::result::Result<alloc::string::String, core::io::error::Error>>::map_err::<alloc::string::String, playground::fat::load<&str>::{closure#0}>
; <core::result::Result<alloc::string::String, core::io::error::Error>>::map_err::<alloc::string::String, playground::fat::load<alloc::string::String>::{closure#0}>
; <core::result::Result<alloc::string::String, core::io::error::Error>>::map_err::<alloc::string::String, playground::fat::load<&std::path::Path>::{closure#0}>
; <core::result::Result<alloc::string::String, core::io::error::Error>>::map_err::<alloc::string::String, playground::fat::load<std::path::PathBuf>::{closure#0}>
; <core::result::Result<alloc::string::String, core::io::error::Error>>::map_err::<alloc::string::String, playground::thin::load::inner::{closure#0}>
```

Four copies for the fat version, one for the thin. The `inner` function is not generic, so its closures aren't either.

In **release**, the picture shifts without changing the conclusion. The four `fat::load` instances were inlined into
`callers`, which grew to 2,269 IR lines, while `thin::load::inner` stayed a single out-of-line function of 530 lines
and the thin shells were inlined as one-line conversions. Inlining didn't remove the fat version's duplication. It moved
the duplication into the callers.

> **Why not just mark it `#[inline]`?** [RUSTC] Generic functions are already available for inlining in every crate that
> uses them (their MIR ships in metadata, Chapter 2.1), so `#[inline]` adds nothing to a generic function except a hint.
> On a *non-generic* function, `#[inline]` makes it behave like a generic one: its code is generated in every crate, and
> in every codegen unit, that uses it. `#[inline]` is a code-size decision, not a speed switch.

**`dyn` at the cold boundary.** Some parameters vary by type but don't need specialized code, such as the sink a report
is written to once a minute. Take `&mut dyn Write` there, and keep generics where the per-record work happens (verified):

```rust
use std::io::{self, Write};

/// Hot path stays generic: called per record, benefits from inlining.
#[inline]
fn encode_field<W: std::fmt::Write>(w: &mut W, key: &str, value: u64) -> std::fmt::Result {
    write!(w, "{key}={value};")
}

/// Cold boundary takes `&mut dyn Write`: one copy of this function, whatever the sink is.
fn flush_report(sink: &mut dyn Write, lines: &[String]) -> io::Result<()> {
    for line in lines {
        sink.write_all(line.as_bytes())?;
        sink.write_all(b"\n")?;
    }
    sink.flush()
}

fn main() -> io::Result<()> {
    let mut lines = Vec::new();
    for (i, v) in [120u64, 7, 9_000].iter().enumerate() {
        let mut s = String::new();
        encode_field(&mut s, "shard", i as u64).unwrap();
        encode_field(&mut s, "p99_us", *v).unwrap();
        lines.push(s);
    }
    let mut file_like: Vec<u8> = Vec::new(); // one sink type
    flush_report(&mut file_like, &lines)?;
    flush_report(&mut io::stdout().lock(), &lines)?; // another sink type, same machine code
    println!("buffered {} bytes", file_like.len());
    Ok(())
}
```

```text
shard=0;p99_us=120;
shard=1;p99_us=7;
shard=2;p99_us=9000;
buffered 59 bytes
```

`flush_report` exists once in the binary. Each `write_all` is an indirect call (Chapter 7.2 showed one in assembly), and
it costs nothing measurable next to the system call it leads to. Projects L1 and L2 made the opposite choice for their
readers (`impl BufRead`), which was right there: the reader is called per line, on the hot path.

**Return-position `impl Trait`: an opaque type.** [LANG] `fn f(..) -> impl Iterator<Item = usize>` returns *one*
concrete type that the body chooses, and hides it from callers. They can use only the trait. It is not dynamic
dispatch: the hidden type is known to the compiler, calls on it are static, and nothing is boxed. It is also not "any
type the caller wants", which is what a generic return parameter `-> T` would mean. Returning two different concrete
types from two branches is a type error. For that you need an enum or `Box<dyn Iterator>`.

The subtle part is **what the hidden type is allowed to borrow.** [VERSION] In edition 2024 (Rust 1.85 and later), an
RPIT is assumed to capture *every* generic parameter in scope, including elided lifetimes. Editions 2021 and earlier
captured type parameters but not lifetimes that don't appear in the bounds. The same function, compiled both ways:

```rust,compile_fail
/// Returns the valid indices of `v`. The hidden type is Range<usize>: it borrows nothing.
fn indices<T>(v: &Vec<T>) -> impl Iterator<Item = usize> {
    0..v.len()
}

fn main() {
    let mut data = vec![10, 20, 30];
    let idx = indices(&data);
    data.push(40); // edition 2024: `idx` is assumed to hold the borrow of `data`
    println!("{} indices, {} items", idx.count(), data.len());
}
```

```text
error[E0502]: cannot borrow `data` as mutable because it is also borrowed as immutable
  --> src/main.rs:11:5
   |
10 |     let idx = indices(&data);
   |                       ----- immutable borrow occurs here
11 |     data.push(40); // edition 2024: `idx` is assumed to hold the borrow of `data`
   |     ^^^^^^^^^^^^^ mutable borrow occurs here
12 |     println!("{} indices, {} items", idx.count(), data.len());
   |                                      --- immutable borrow later used here
   |
note: this call may capture more lifetimes than intended, because Rust 2024 has adjusted the `impl Trait` lifetime capture rules
  --> src/main.rs:10:15
   |
10 |     let idx = indices(&data);
   |               ^^^^^^^^^^^^^^
help: use the precise capturing `use<...>` syntax to make the captures explicit
   |
 4 | fn indices<T>(v: &Vec<T>) -> impl Iterator<Item = usize> + use<T> {
   |                                                          ++++++++
```

Under edition 2021, the same file compiles and prints `3 indices, 4 items` (verified with an edition override). The
2024 default is the safer one for API evolution. A function that captures everything today can later return a type that
really borrows, without breaking callers. When you want to promise *less*, say so with `use<..>` (stable since 1.82).
Verified:

```rust
/// `use<T>` lists exactly what the opaque type may capture: T, but not the borrow's lifetime.
fn indices<T>(v: &Vec<T>) -> impl Iterator<Item = usize> + use<T> {
    0..v.len()
}

/// The opposite case: the hidden type really does borrow `v`, so it must capture its lifetime.
fn positive<'a>(v: &'a [i64]) -> impl Iterator<Item = i64> + use<'a> {
    v.iter().copied().filter(|&x| x > 0)
}

fn main() {
    let mut data = vec![10, 20, 30];
    let idx = indices(&data);
    data.push(40); // fine: the signature promises `idx` holds no borrow
    println!("{} indices, {} items", idx.count(), data.len());

    let deltas = [-5, 7, 0, 12];
    let pos: Vec<i64> = positive(&deltas).collect();
    println!("positive deltas: {pos:?}");
}
```

```text
3 indices, 4 items
positive deltas: [7, 12]
```

`use<..>` is part of the function's **public contract**. Once published, removing a lifetime from it is fine, but
adding one is a breaking change. [VERSION] At stabilization, `use<..>` had to list every *type* parameter in scope, and
only lifetimes could be left out. Check the current rule before relying on omitting a type parameter.

**One more `impl Trait` contract detail: argument position hides the parameter.** A caller cannot turbofish an
`impl Trait` argument (verified):

```text
error[E0107]: function takes 0 generic arguments but 1 generic argument was supplied
  --> src/main.rs:11:20
   |
11 |     println!("{}", log_field::<u16>("port", 8080)); // there is no parameter you can name here
   |                    ^^^^^^^^^------- help: remove the unnecessary generics
   |                    |
   |                    expected 0 generic arguments
   |
note: function defined here, with 0 generic parameters
  --> src/main.rs:5:4
   |
 5 | fn log_field(name: &str, value: impl Display) -> String {
   |    ^^^^^^^^^
   = note: `impl Trait` cannot be explicitly specified as a generic argument
```

That makes `fn f<T: Display>(x: T)` and `fn f(x: impl Display)` different *APIs* even though they generate identical
code. §13 turns this into a semver question.

---

## Pass 2 · Systems level — *Where the build time and the bytes go*

### 4. Under the hood

**Front end once, back end per instance.** [RUSTC] Parsing, name resolution, type checking, and borrow checking run
once per generic *definition* (Chapter 7.1). Everything after the monomorphization collector runs per *instance*: MIR
substitution, LLVM IR generation, LLVM's optimization passes, and machine-code emission. For optimized builds, LLVM
optimization is usually the largest single phase, and its work grows with the amount of IR it receives. That is why the
table in §3 counted IR. [RUSTC] As a proxy it is imperfect (some functions optimize faster than others), but it is the
same proxy the community tooling uses.

**Codegen units and parallelism.** [RUSTC] The collector's mono items are partitioned into codegen units, which LLVM
compiles in parallel. The defaults are 16 CGUs for non-incremental builds and 256 for incremental ones. More CGUs mean
more parallelism, but less inlining across CGU boundaries. That is why Meridian's gateway release profile uses
`codegen-units = 1` (Chapter 2.2): it traded build time for optimization. `#[inline]` functions are copied into *each*
CGU that uses them, so they multiply by CGUs as well as by crates.

**Where the instance is compiled.** [RUSTC] An instance is generated in the crate that needs it, usually the one that
supplies the concrete type (Chapter 2.1's "the recipe ships, your crate cooks"). The consequence for crate splits: moving
a generic library into its own crate makes its *definitions* compile in parallel, but its *instances* still compile in
the downstream crate that uses them, usually the binary at the end of the dependency graph. Splitting crates speeds up
non-generic code and type checking far more than it speeds up monomorphization. [RUSTC] In unoptimized builds, rustc
lets a crate reuse instances already exported by its dependencies ("shared generics", on by default for debug builds),
which removes some duplicates. Optimized builds generate their own copies so that they can inline them.

**`cargo check` skips all of it.** [RUSTC] `cargo check` runs the front end and writes metadata (Chapter 2.1). No
collector, no LLVM, no linking. For the edit loop, that removes the monomorphization cost completely. Its blind spot is
the one Chapter 7.1 showed: post-monomorphization errors need a real build.

**Measuring instead of guessing.** Four tools, in order of how much this book has verified them:

- **Counting IR yourself** (verified; what this chapter did): emit LLVM IR (`cargo rustc -- --emit=llvm-ir`, or the
  Playground), count `define` lines, and attribute each to a generic function by its demangled name.
- **`cargo build --timings`** (stable Cargo, not run here: the Playground doesn't expose it) writes an HTML report of
  per-crate compile times, showing which crates are on the critical path and whether codegen or the front end dominates.
- **`cargo llvm-lines`** (a third-party Cargo subcommand; **not verified in this book**, since the Playground can't run
  it) automates the counting: it reports IR lines and copies per generic function, summed over instances. Its output
  would list `top_n`'s dependencies near the top in the 16-type build above.
- **`-Z self-profile` and `-Z time-passes`** (nightly rustc flags, [VERSION]) break a crate's compile time down by
  compiler phase and query. Part XVIII, *How rustc Works*, uses them.

**Faster back ends and front ends.** [VERSION] The Cranelift code generator is available as a rustup component on
nightly for debug builds. It trades run-time speed for much faster code generation, and it attacks exactly the
per-instance cost. A parallel front end exists on nightly (Chapter 1.4). Check both before depending on them. Chapter
18.6, *Monomorphization and Codegen Backends (LLVM, Cranelift)*, goes inside both.

### 5. Memory

**Binary size is instance count times instance size.** In the growth experiment, release assembly went from 2,116 to
26,082 instruction lines, about 1,600 per row type. On x86-64, instructions average a few bytes each, so the program's
code grew by several kilobytes per type (an estimate from typical instruction lengths, not a measured section size). For
one helper that is nothing. For a codebase with hundreds of generic helpers, dozens of types, and debug info, it adds up
to the multi-megabyte binaries people complain about.

**Debug builds pay more.** Every debug instance is out of line and carries debug info. The debug IR text for the
16-type listing was about 20 MB, against about 1.8 MB for one type (the Playground's output sizes). Debug binaries of
generic-heavy services are commonly several times larger than their release builds for this reason.

**Compiler memory.** [RUSTC] rustc and LLVM hold the IR for a codegen unit in memory while optimizing it, so peak
compiler memory also grows with instance count. On small CI runners, a large generic-heavy binary crate can fail
through memory pressure (OOM-killed linkers and rustc processes) before it fails on time.

**Size tools, not habits.** [LIB] Release-profile settings trade size against speed: `opt-level = "s"`/`"z"`,
`lto`, `codegen-units = 1`, `panic = "abort"`, and `strip` (Chapter 2.2 covers profiles). None of them removes an
instance your code asked for. The levers in this chapter act on the cause.

### 6. CPU / OS

**The build is a parallel program with a serial tail.** [OS] Cargo builds independent crates in parallel. But the final
binary crate, where most instances of your generics end up, starts only after its dependencies finish, and it is often
the longest single job. Within it, parallelism comes only from codegen units. Moving instances out of the binary crate
(the inner-function pattern pushes the body into the crate that *defines* it, because `inner` isn't generic) shortens
the serial tail. That is a stronger argument for the pattern than binary size.

**Linking.** [VERSION] More instances mean more symbols and relocations for the linker. Since Rust 1.90 the default
linker on `x86_64-unknown-linux-gnu` is `lld` (Chapter 1.4), which helps with the link step but not with codegen.

**Run time: the instruction cache, again.** [CPU] Chapter 7.1 noted that duplicated hot code competes for L1i.
De-genericizing a *cold* path is free at run time and saves build time. De-genericizing a *hot* path trades build time
for an indirect call and lost inlining, and should be measured (Part XX).

---

## Pass 3 · Architect level — *Budgets, boundaries, and API contracts*

### 7. Trade-offs

| Technique | Reduces | Run-time cost | Use when | Evidence here |
|---|---|---|---|---|
| Inner non-generic function | Size of each instance; closures stop multiplying | One extra direct call (usually inlined) | A generic parameter is converted immediately (`AsRef`, `Into`, `impl Read`) | 88 → 26 debug IR functions |
| Concrete parameter types | Instance count to 1 | Callers convert (`&str`, `&Path`, `&[T]`) | Internal APIs; the flexibility isn't used | — |
| `&dyn Trait` / `Box<dyn Trait>` | Instance count to 1 | Indirect call; no inlining across it | Cold paths, I/O sinks, plugins | 7.2's vtable call |
| Enum of known types | Instance count to 1 | A `match` per call | Closed set of types | — |
| Fewer generic parameters | Combinatorial instances (`A×B×C`) | Depends | Parameters that are really configuration | — |
| `cargo check` in the loop | Edit-loop time | None | Always | — |
| Crate splits | Front-end and non-generic codegen time; better caching | None | Large workspaces | — |
| Cranelift (debug) | Codegen time | Slower debug binaries | Nightly-tolerant teams | Not verified |

The combinatorial row deserves attention. A `Client<T: Transport, S: Serializer, R: RetryPolicy>` with 3 transports, 2
serializers, and 4 retry policies has up to 24 instances of *every* method, and each instance also instantiates what
those methods call. Parameters that select behavior at configuration time, such as the retry policy, are usually better
as values (`Box<dyn RetryPolicy>` or an enum) than as types.

### 8. Java comparison

**Java pays the same bill at a different time.** [RUNTIME] `javac` is fast because it barely compiles. The JIT does the
equivalent of monomorphization and inlining at run time, and it has a code-size budget too. HotSpot's compiled code
lives in the **code cache** (`-XX:ReservedCodeCacheSize`), and a JVM that fills it logs "CodeCache is full. Compiler has
been disabled." and keeps running interpreted, which is a latency incident. C2 also limits inlining by bytecode size
(the `MaxInlineSize` and `FreqInlineSize` flags; check their defaults on your JDK with `-XX:+PrintFlagsFinal`). The JVM's
"generic bloat" is bounded by those budgets, applied adaptively to hot code only. Rust compiles every instance, hot or
cold, ahead of time.

**Build structure maps imperfectly.** Maven or Gradle modules resemble crates, and annotation processors resemble
procedural macros. But in Java, a module's generic code is compiled once, in that module. In Rust, a crate's generic
code is compiled again in every crate that instantiates it.

**The inner-function pattern has a Java cousin, for a different reason.** Java engineers extract a method from a large
generic method so that the hot part fits C2's inlining budget. Rust engineers extract a non-generic core so that the
cold part is compiled once. The refactoring is the same, but it addresses opposite costs.

> **Analogy limit.** "Splitting a Rust crate is like splitting a Maven module" holds for non-generic code: each crate
> compiles independently, in parallel, and is cached. It breaks for generics. A generic function in `meridian-telemetry`
> costs nothing when `meridian-telemetry` builds, and costs its instances in *every* service that uses it. In Java terms,
> it is as if every consumer of a library recompiled the library's generic methods for its own types.

### 9. Production scenario

**Meridian's gateway workspace goes on a diet.** The gateway workspace (Chapter 2.7: `gateway-core`, `gateway-proto`,
`gateway-io`, and the `gateway` binary) had grown a middleware stack generic over four parameters:
`Pipeline<T: Transport, C: Codec, M: Metrics, L: Limiter>`. It was written that way "for testability", with each test
substituting fakes. Engineers complained that a one-line change in `gateway-core` meant a long wait for the `gateway`
crate to rebuild. `cargo build --timings` showed the binary crate's codegen as the critical path: all the pipeline's
instances were compiled there.

The team counted IR per generic function, as in §3 (they also used `cargo llvm-lines` locally), and made four changes:

1. **Metrics and the limiter became values.** There was one production implementation of each, and the tests' fakes
   were simple. They became `Arc<dyn Metrics>` and an enum `Limiter { TokenBucket(..), Disabled }`. The metrics calls
   were not in the per-byte path, and the indirect call was invisible in the gateway's latency benchmark.
2. **Transport and codec stayed generic.** They are on the per-request path, and the codec's inlined parsing was worth
   keeping (Chapter 2.4's frame parser).
3. **Every `AsRef`/`Into` entry point got a thin shell.** Config loading, route-table building, and TLS file loading
   were rewritten in the §3 pattern.
4. **The edit loop moved to `cargo check`**, with the full test suite run before each commit.

The durable result was a process, not a one-off cleanup. CI now emits LLVM IR for the `gateway` crate on each merge,
records the total line count and the top generic functions by IR, and fails the build if the total grows by more than
a set percentage without a label on the PR. **Code size became a reviewed budget,** like latency.

### 10. Failure scenario

**The edition migration that added a clone per request.** Meridian's session manager (Part IV's review capstone) moved to edition
2024 by editing `edition = "2024"` in `Cargo.toml` by hand, instead of running `cargo fix --edition`. [VERSION] That
command applies the migration lint for exactly this rule change (`impl_trait_overcaptures`), which would have added
`use<..>` bounds where the old capture behavior was needed. The build broke with E0502 at fourteen call sites of a helper
shaped like `indices(&Vec<T>) -> impl Iterator` (§3's error). The developer on call for the migration "fixed" each call
site the fastest way they found:

```rust,ignore
// The "fix" that shipped (sketch): borrow a clone, so the original can be mutated.
let idx = session_ids(&sessions.clone());   // allocates and copies the Vec on every request
sessions.push(new_session);
```

Every request now cloned a `Vec` of session IDs, a few thousand bytes, to satisfy a borrow the hidden type never
actually held. The allocation profile regressed, and p99 moved enough for the latency SLO dashboard to flag the deploy.
The compiler had offered the right fix in the error itself: `help: use the precise capturing use<...> syntax`, one
edit to one signature.

Two lessons. **Read the whole diagnostic**, especially `note:` and `help:` lines that mention an edition. And **run
the edition migration tooling instead of flipping the edition by hand**: the migration lints exist to express the old
behavior in the new edition. The post-incident action item added `cargo fix --edition` plus a review of every inserted
`use<..>` to the team's edition-upgrade runbook, because each `use<..>` is a public API promise (§3).

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part VII).*

1. Why does the cost of a generic function scale with "body × instantiations", and why is "body" larger than the
   function's source? Use the growth table.
2. Explain the non-generic inner function pattern. Why did it reduce the debug IR from 88 functions to 26, and not just
   from 4 to 1?
3. Why are closures inside a generic function generic, even when they don't mention the type parameter?
4. Where is a generic function's instance compiled when the definition is in crate A and the type is in crate B? What
   does that imply for splitting crates?
5. What does `cargo check` skip, and what can it therefore miss?
6. What is return-position `impl Trait`? How does it differ from `-> T` and from `-> Box<dyn Trait>`?
7. What changed about RPIT lifetime capture in edition 2024, why is the new default safer for library evolution, and
   what does `use<..>` do?
8. How would you set a CI budget for code size? What would you measure, and what would you allow to exceed it?

### 12. Exercises

- **Beginner.** Rewrite `fn read_lines<P: AsRef<Path>>(p: P) -> io::Result<Vec<String>>` from a fat to a thin
  version. Name the one line that stays generic.
- **Intermediate.** In `ch03-01-inner-fn.rs`, change `fat::load`'s two closures into named non-generic functions
  (`fn describe(path: &Path, e: io::Error) -> String`, …), keeping the body generic. Re-count the debug IR. How much of
  the 22-per-type cost was due to the closures?
- **Advanced.** Take the 16-type growth listing and replace `sort()` with `sort_unstable()`, then with a
  `sort_by_key` over a non-generic key extractor. Measure debug and release IR for each and explain the differences.
- **Systems.** Emit release IR for the growth listing with row types of *different* layouts (`(u32, String)`, `u64`,
  `[u8; 3]`, …). Does LLVM still merge the small sort helpers? Explain what you find.
- **Architecture.** Pick a generic-heavy crate your team depends on (web framework, ORM, serializer). Estimate which
  of its generic functions your service instantiates and how many times. Propose one change to your code, not theirs,
  that would reduce it, and name how you'd measure the effect.

### 13. Debugging exercise

`meridian-log` 1.4.0 has this function, and downstream services call it as `log_field::<u16>("port", p)` to force the
formatting of narrowed integers:

```rust,ignore
pub fn log_field<V: Display>(name: &str, value: V) -> String { format!("{name}={value}") }
```

Version 1.4.1, a "cleanup" release, changes it to `pub fn log_field(name: &str, value: impl Display) -> String`.

1. Predict what happens to the downstream build, with the error code. (The error is verified in
   `listings/part-07/ch03-07-apit-turbofish.rs`.)
2. The generated code is identical in both versions. Why is this still a breaking change, and which part of semver does
   it violate?
3. When is argument-position `impl Trait` the better choice for a *new* public function, and when is an explicit
   parameter better?

### 14. Design exercise

**An instantiation budget for `meridian-telemetry`.** A shared telemetry crate is used by 30 Rust services. Its API
today: `counter<N: Into<Cow<'static, str>>>(name: N)`, `record<V: Into<f64>, L: IntoIterator<Item = (K, K)>, K:
AsRef<str>>(name, value: V, labels: L)`, and a `Exporter<T: Transport, E: Encoder>` type with 20 methods.

Redesign it with a stated budget: at most N instances per service of any function in the crate, and no generic function
body longer than a thin shell unless it is on a measured hot path. Decide which parameters become concrete, which
become `dyn`, which become values, and which stay generic. Specify the CI check that enforces the budget in each of the
30 services, and what a team must show to get an exception. Which trade-off in §7 do you expect to be the most
controversial with the teams that use the crate, and why?
