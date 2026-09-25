# Chapter 1.2 — Five Languages, Five Bets

> **Where this sits:** Part I · Why Rust Exists · chapter 2 of 4
> **Prerequisites:** Chapter 1.1 (the lifecycle, the taxonomy, the central question).
> **After this chapter you can:** place C, C++, Java, Go, and Rust in the design space by *who pays for what, and when*;
> describe what each one puts inside your process; and explain the memory-layout consequences a Java engineer usually
> doesn't see.

---

## Pass 1 · User level — *What did each language bet on?*

### 1. Problem

Feature-list comparisons ("Rust has pattern matching, Go has goroutines") don't help an architect. Every language is a
**bundle of bets** about which costs to pay, when, and who pays them. A language that makes the programmer's life easy
pushes cost somewhere else: into the runtime, into production incidents, or into the hardware bill. A language that
makes the machine's life easy pushes cost back onto the programmer and the compiler.

To judge Rust you need to see the whole design space, including the parts where Rust is the worse choice.

### 2. Mental model

**Four parties can pay for safety and performance:**

```text
   THE PROGRAMMER         THE COMPILER            THE RUNTIME               PRODUCTION
   (writing time)         (build time)            (every execution)         (incidents)
         │                      │                        │                        │
   annotations,           type checking,           GC, JIT, bounds          crashes, CVEs,
   discipline,            borrow checking,         checks, scheduler,       data corruption,
   design effort          monomorphization         write barriers           on-call pages
```

Every language moves cost between these columns. **C** puts almost everything on the programmer and, when the
programmer slips, on production. **Java** puts it on the runtime. **Rust** puts it on the programmer and the compiler, up
front, so the runtime and production pay less.

Here is the design space, drawn qualitatively. Positions are judgments, not measurements:

```text
                         ENFORCED SAFETY (memory + data races)
                                        ▲
                                        │                           ● Rust
                   Java ●               │                   (compile-time proof,
          (VM: GC + JIT; memory-safe    │                    no runtime; data-race-
           even under data races)       │                    free in safe code)
                                        │
                              Go ●      │
                (GC in a small runtime; │
                 races can break safety)│
  ──────────────────────────────────────┼──────────────────────────────────────────►
                                        │               CONTROL & PREDICTABILITY
                                        │               (layout, allocation, latency)
                                        │                     ● C++
                                        │            (RAII, smart pointers: safety
                                        │             by convention; UB remains)
                                        │                         ● C
                                        │               ("trust the programmer")
```

Rust's claim is the top-right corner, which until 2015 was mostly empty in mainstream practice. Research languages had
explored nearby (Cyclone's region-based memory management was a direct influence), but none had become an industrial
tool.

**The five bets, one sentence each:**

```text
1972  C      "The programmer knows best. The language is portable assembly."
             (The C Rationale's 'spirit of C': trust the programmer; make it fast even if not portable.)

1985  C++    "Abstraction without overhead: what you don't use, you don't pay for, and what you do use,
             you couldn't hand-code any better."  (Stroustrup's zero-overhead principle.)

1995  Java   "A managed runtime makes programs safe and portable; the runtime (JIT + GC) will make them fast."

2009  Go     "Simplicity at scale: a small language, fast builds, GC, and cheap concurrency for networked
             services written by large teams."  (Public 2009, Go 1.0 in 2012.)

2015  Rust   "Safety and control are not a trade-off if the compiler can prove lifetime and aliasing facts.
             Pay at compile time, not at run time."  (Mozilla-sponsored from 2009; Rust 1.0 in May 2015.)
```

### 3. Rust code

One question separates these languages cleanly: **what happens when a value must outlive the stack frame that created
it?** Watch each language answer it.

**C** lets you return a pointer to a dead stack slot:

```c
int *make(void) {
    int x = 42;
    return &x;          // compiles (usually with a warning); using the result is undefined behavior
}
```

Compilers warn about this simple case. Slightly less direct versions, like storing `&x` in a struct or passing it out
through a pointer parameter, often compile silently.

**Go** quietly moves the variable to the heap:

```go
func newAnswer() *int {
    x := 42
    return &x           // fine: escape analysis sees &x escape and heap-allocates x
}                       // (`go build -gcflags=-m` prints "moved to heap: x")
```

It's safe, it's convenient, and the allocation is invisible at the call site. That makes it cheap to write and sometimes
expensive to run.

**Java** has no way to take the address of a local. Objects always live on the GC heap:

```java
static int[] make() {
    int[] x = {42};     // arrays and objects live on the GC heap*
    return x;           // fine: the GC keeps it alive while referenced
}
// *HotSpot may scalar-replace objects that provably don't escape. That optimization is invisible to semantics.
```

**Rust** refuses, and makes you choose:

```rust,compile_fail
fn make<'a>() -> &'a i32 {
    let x = 42;
    &x
}
```

```text
error[E0515]: cannot return reference to local variable `x`
 --> src/main.rs:4:5
  |
4 |     &x
  |     ^^ returns a reference to data owned by the current function
```

The fixes are explicit decisions: return the value (`fn make() -> i32`), or put it on the heap yourself
(`fn make() -> Box<i32>`). Rust won't allocate behind your back the way Go does, and it won't let you dangle the way
C does.

What Rust *does* allow is returning a reference into data the **caller** owns, with the relationship written into the
signature:

```rust
fn largest<T: PartialOrd>(items: &[T]) -> Option<&T> {
    let mut best = items.first()?;
    for item in items {
        if item > best {
            best = item;
        }
    }
    Some(best)
}

fn main() {
    let scores = vec![3, 9, 4];
    println!("{:?}", largest(&scores));
    let empty: Vec<i32> = Vec::new();
    println!("{:?}", largest(&empty));
    let words = ["kiwi", "apple", "mango"];
    println!("{:?}", largest(&words));
}
```

```text
Some(9)
None
Some("mango")
```

The signature `fn largest<T>(items: &[T]) -> Option<&T>` reads, in full: *"the returned reference points into `items`
and is valid for as long as `items` is borrowed."* Nobody wrote a lifetime, because Rust's **elision rules** fill in the
obvious one. [LANG] With exactly one reference input, the output reference gets that input's lifetime. The return type
is `Option<&T>` rather than a nullable pointer, so "empty input" is a value the caller must handle. It isn't a null to
forget about.

That contract is enforced at every call site:

```rust,compile_fail
// `largest` as defined above
fn main() {
    let top;
    {
        let scores = vec![3, 9, 4];
        top = largest(&scores);
    } // `scores` is dropped here
    println!("{top:?}");
}
```

```text
error[E0597]: `scores` does not live long enough
   |
15 |         let scores = vec![3, 9, 4];
   |             ------ binding `scores` declared here
16 |         top = largest(&scores);
   |                       ^^^^^^^ borrowed value does not live long enough
17 |     } // `scores` is dropped here
   |     - `scores` dropped here while still borrowed
18 |     println!("{top:?}");
   |                --- borrow later used here
```

The same question gets four answers:

| Language | Value must outlive its frame | Cost | Failure mode |
|---|---|---|---|
| C | Programmer's problem | None | UB, silent |
| Go | Compiler moves it to the heap automatically | Hidden allocation + GC work | None (safe), but allocation is invisible |
| Java | Everything is already on the heap | Allocation + GC work (mitigated by escape analysis) | None (safe) |
| Rust | Programmer chooses: move by value, `Box`, or borrow with a checked lifetime | Visible, chosen by you | Compile error |

---

## Pass 2 · Systems level — *What does each language put inside your process?*

### 4. Under the hood

The biggest structural difference between these languages is **how much code runs in your process that you didn't
write**:

```text
 C / C++ / Rust binary          Go binary                          Java process
┌──────────────────────┐   ┌───────────────────────────────┐   ┌────────────────────────────────────┐
│ your code            │   │ your code                     │   │ the JVM (native code):             │
│                      │   │                               │   │  ├─ class loader + verifier        │
│ std library          │   │ Go runtime, linked in:        │   │  ├─ bytecode interpreter           │
│  (+ libc)            │   │  ├─ scheduler (G-M-P)         │   │  ├─ JIT compilers (C1, C2)         │
│                      │   │  ├─ concurrent GC             │   │  ├─ GC (G1, ZGC, Shenandoah, ...)  │
│ allocator            │   │  ├─ growable goroutine stacks │   │  ├─ safepoints, deoptimization     │
│                      │   │  ├─ netpoller (epoll/kqueue/  │   │  └─ JVMTI, JFR, JMX, agents        │
│ panic / unwind       │   │  │   IOCP)                    │   │                                    │
│ support              │   │  └─ preemption via signals    │   │ your bytecode + JDK class library  │
└──────────────────────┘   └───────────────────────────────┘   └────────────────────────────────────┘
   "no runtime"               "runtime compiled in"              "runtime is the platform"
```

**C.** Essentially nothing beyond startup code (`_start` → libc initialization → `main`) and libc itself.

**C++.** Adds static initializers, exception-handling tables (unwinding metadata that costs nothing until something
throws), RTTI, and the standard library. Still no scheduler and no GC.

**Java.** The JVM *is* the product. Code starts out interpreted, is profiled, and then gets compiled by a tiered JIT:
C1 for fast compilation, C2 for heavily optimized code built on **speculative** assumptions taken from the live profile
("this interface call has only ever seen one class, so inline it"). If an assumption breaks, the JVM **deoptimizes**
back to the interpreter. The GC runs concurrently on its own threads, and application threads cooperate through
**safepoints** and read/write **barriers**. That's a lot of sophisticated machinery working for you, and it's also why
JVM performance engineering is largely about tuning that machinery.

**Go.** Compiles ahead of time to native code, but links a runtime into every binary. That runtime includes the
**G-M-P scheduler** (goroutines G multiplexed onto OS threads M through logical processors P, with work stealing), a
**concurrent, non-moving, non-generational mark-sweep GC**, goroutine stacks that start at a few KB and grow by copying,
and a **netpoller** that turns blocking-style network code into epoll/kqueue/IOCP events. The GC's pacer targets about
25% of CPU for background marking. When a goroutine allocates faster than marking keeps up, the runtime makes it do
**mark assists**, so GC cost can show up as latency in your request path.

**Rust.** Like C, it has no GC and no scheduler. [RUNTIME] Before `main`, std does a little setup: on Unix-like
platforms it installs a handler so that a stack overflow prints a message instead of dying with a bare segfault, and it
records the program's arguments. After `main`, a panic that escaped becomes exit code 101. `#![no_std]` programs (kernels,
firmware) drop even that. **Async runtimes such as Tokio are ordinary libraries**, which you choose and link in. They
aren't part of the language.

> **What actually happens?** Rust *used to* look much more like Go. Before 1.0, Rust had garbage-collected `@` pointers,
> green threads multiplexed by a runtime scheduler, and segmented stacks. All three were removed on purpose.
> Segmented stacks were abandoned in 2013 because of the "hot split" problem, where a function call at a segment
> boundary in a hot loop repeatedly allocated and freed a segment. Go hit the same problem and moved to contiguous,
> copyable stacks in Go 1.3. The green-thread runtime was removed from std in 2014 (RFC 230). The reason was
> **embeddability**. A language with a mandatory runtime can't be dropped into a C program, a browser engine, an OS
> kernel, or a Python extension without bringing its scheduler and GC along. Rust gave up built-in green threads so it
> could go anywhere C can go. Async/await, stabilized in 2019, brought lightweight concurrency back as a library-level
> feature (Part XII).

### 5. Memory

Layout is where a Java engineer's intuition most often misleads them. Here are sizes as rustc reports them on 64-bit
Linux (verified output):

```rust
use std::mem::size_of;

#[allow(dead_code)]
struct Point {
    x: f64,
    y: f64,
}

fn main() {
    println!("Point:             {:>2} bytes", size_of::<Point>());
    println!("[Point; 4]:        {:>2} bytes", size_of::<[Point; 4]>());
    println!("Vec<Point>:        {:>2} bytes (the handle, not the elements)", size_of::<Vec<Point>>());
    println!("Box<Point>:        {:>2} bytes", size_of::<Box<Point>>());
    println!("&u64:              {:>2} bytes", size_of::<&u64>());
    println!("Option<&u64>:      {:>2} bytes", size_of::<Option<&u64>>());
    println!("Option<Box<u64>>:  {:>2} bytes", size_of::<Option<Box<u64>>>());
    println!("u64:               {:>2} bytes", size_of::<u64>());
    println!("Option<u64>:       {:>2} bytes", size_of::<Option<u64>>());
    println!("String:            {:>2} bytes", size_of::<String>());

    let points: Vec<Point> = (0..1_000_000)
        .map(|i| Point { x: i as f64, y: 0.0 })
        .collect();
    let payload = points.len() * size_of::<Point>();
    println!("1M points payload: {} bytes, one contiguous block", payload);
}
```

```text
Point:             16 bytes
[Point; 4]:        64 bytes
Vec<Point>:        24 bytes (the handle, not the elements)
Box<Point>:         8 bytes
&u64:               8 bytes
Option<&u64>:       8 bytes
Option<Box<u64>>:   8 bytes
u64:                8 bytes
Option<u64>:       16 bytes
String:            24 bytes
1M points payload: 16000000 bytes, one contiguous block
```

What each line tells you:

- **No object header.** A `Point` is exactly its two `f64`s. Type information lives in the compiler, not in memory.
- **`Option<&u64>` is the same size as `&u64`.** [LANG] This is a documented guarantee (the "null pointer
  optimization"). A reference can never be null, so `None` can use the all-zero bit pattern. You get null-safety in the
  type system at zero space cost.
- **`Option<u64>` costs 16 bytes**, because every `u64` bit pattern is a valid number, so the discriminant needs its own
  space, padded to alignment. Part V covers these **niches** in depth.
- **`String` and `Vec` are 24-byte handles** (pointer, capacity, length) that point to heap storage. [LIB] Three words is
  the current layout on 64-bit targets. std doesn't promise the field order.

Now the same million points in Java and Go:

```text
Rust  Vec<Point>   /  C++ std::vector<Point>   /   Go []Point
  handle: ptr | cap | len
  heap:  [x0 y0][x1 y1][x2 y2][x3 y3] ... [x999999 y999999]     16 B each, contiguous  =  16 MB

Java  Point[]  (typical 64-bit HotSpot: compressed oops and class pointers, heap < 32 GB)
  heap:  [hdr 16B][ref0][ref1][ref2][ref3] ...                  4 B per reference      ≈  4 MB
                     │     │     │
                     ▼     ▼     ▼
                   [hdr 12B | x 8B | y 8B | pad 4B]  × 1,000,000  32 B each           ≈ 32 MB
                   (placed wherever allocation and GC put them)
                                                                          total        ≈ 36 MB
```

The Java figure assumes the common 12-byte object header. With compact object headers (Project Lilliput, experimental
in JDK 24 and a product option in JDK 25) the header shrinks to 8 bytes and each `Point` to 24. That's better, but the
reference indirection is still there. Project Valhalla's value classes aim to remove the indirection itself. Check
their status in your JDK before counting on them.

Two honest corrections to the usual "Java is bloated" story:

1. **Allocation in Java is extremely cheap.** It's a pointer bump in a thread-local allocation buffer (TLAB), usually
   cheaper than `malloc`. Consecutively allocated objects often land next to each other, and copying collectors often
   preserve that order. The locality penalty is frequently much smaller than the diagram suggests. It's still *the
   runtime's* behavior, though, and you don't control it.
2. **High-performance Java already knows this.** It uses primitive arrays in struct-of-arrays form (`double[] xs;
   double[] ys;`) or off-heap memory (`MemorySegment` in the FFM API) to get contiguous layout. It's possible. It just
   isn't the default, and it gives up the object model.

**Go** sits in between. Structs are values, so `[]Point` is contiguous like Rust. You opt into indirection with
`[]*Point`. And because a `[]Point` of floats contains no pointers, Go's GC can skip scanning it entirely. Pointer-free
data is cheap for a tracing GC, which is worth remembering whenever you design data for any GC'd language.

### 6. CPU / OS

| | C / C++ | Java | Go | Rust |
|---|---|---|---|---|
| **Thread model** | 1:1 OS threads | 1:1 platform threads; M:N **virtual threads** (JDK 21+) | M:N goroutines on OS threads | 1:1 OS threads; M:N async tasks via library runtimes |
| **Stack per unit of concurrency** | OS default; glibc takes new-thread stacks from `RLIMIT_STACK`, commonly 8 MiB | ~1 MiB per platform thread (`-Xss`); virtual-thread stacks are growable heap chunks | Starts at a few KB, grows by copying | [LIB] 2 MiB default for spawned threads (configurable); **async tasks have no stack**, they're state machines sized at compile time |
| **GC work at run time** | None | Concurrent GC threads, barriers, safepoints | Concurrent GC (~25% CPU target during marking), mark assists | None |
| **Code generation** | AOT | **JIT** (tiered, speculative, profile-driven); AOT options exist | AOT, fast compiler, fewer optimizations | AOT via LLVM; PGO and BOLT optional |
| **Preemption** | OS | OS, plus safepoint polls | OS for threads; runtime preempts goroutines asynchronously via signals (Go 1.14+) | OS for threads; async tasks are **cooperative** and yield only at `.await` |

Three consequences worth internalizing:

- **Stackful vs stackless concurrency.** A goroutine or a Java virtual thread has a real (growable) stack, so any
  function can block anywhere. A Rust async task is a compiler-generated state machine whose size is the maximum state
  live across any `.await`. That's often a few hundred bytes, known at compile time. It's extremely memory-efficient,
  but blocking inside it blocks an executor thread. That trade-off drives most of Part XII.
- **A JIT can beat AOT.** HotSpot's C2 inlines through interface calls it has *observed* to be monomorphic and
  deoptimizes if that changes. An AOT compiler must either prove monomorphism or guess. Rust narrows the gap by
  **defaulting to static dispatch** (generics are monomorphized, so there's nothing to speculate about) and supports
  profile-guided optimization when you need it. Part VII covers monomorphization; Part XX covers PGO.
- **Idle overhead differs by an order of magnitude.** An idle JVM runs a dozen or more threads (GC workers, JIT
  compilers, reference handler, signal dispatcher, and others). An idle Rust binary runs exactly the threads you
  created. For a sidecar deployed 10,000 times, that difference turns into real money.

---

## Pass 3 · Architect level — *Which bet fits which workload?*

### 7. Trade-offs

The full comparison. It's qualitative on purpose: several rows depend heavily on the workload, and later Parts measure
the ones that matter.

| Dimension | C | C++ | Java | Go | Rust |
|---|---|---|---|---|---|
| **Enforced memory safety** | No | No (tools help) | Yes | Yes, except under data races | Yes, in safe code |
| **Data-race freedom** | No | No | No (races don't break memory safety) | No (race detector at test time) | Yes, in safe code |
| **UB exposure** | Pervasive | Pervasive | Essentially none | Via races and `unsafe` | Only inside `unsafe` |
| **Memory management** | Manual | RAII + smart pointers | Tracing GC | Tracing GC | Ownership + RAII (opt-in RC) |
| **Runtime in process** | libc | libc + C++ runtime | Full VM | Scheduler + GC compiled in | Minimal std, or none (`no_std`) |
| **Latency predictability** | High | High | Good, with tuning (GC, JIT warm-up) | Good (short pauses; GC CPU and assists) | High |
| **Startup** | Instant | Instant | Slow-ish (class loading, warm-up); mitigations exist | Fast | Instant |
| **Memory footprint** | Minimal | Minimal | Largest (headroom, metadata) | Moderate | Minimal |
| **Layout control** | Full | Full | Limited by default | Good (value structs) | Full |
| **Cost of abstraction** | Few abstractions to pay for | Zero-overhead templates | Depends on the JIT | Moderate (interfaces, escape analysis) | Zero-overhead generics; `dyn` when you ask |
| **Generics** | Macros, `void*` | Templates (monomorphized) | Erased | GC-shape stenciling + dictionaries | Monomorphized, bounds checked at definition |
| **Error model** | Return codes, `errno` | Exceptions (+ codes) | Exceptions (checked + unchecked) | Error values | `Result` + `panic!` for bugs |
| **Null** | `NULL` | `nullptr` | `null` | `nil` | No null references; `Option<T>` |
| **Compile speed** | Fast | Slow | Fast (javac); the JIT works at run time | Very fast | Slow |
| **Learning curve** | Small language, hard to use safely | Very large | Moderate | Small | Steep |
| **Backend ecosystem** | Thin | Moderate | Vast | Large | Growing, strong in infrastructure |
| **Production introspection** | gdb, perf | gdb, perf | Excellent (JFR, heap dumps, agents) | Good (pprof, trace) | Good (perf, tracing); less runtime introspection |

Read the table by **columns of cost**, not rows of features. Java pays at run time and in memory, and buys programmer
productivity and deep observability with it. Go pays some runtime cost and some expressiveness, and buys simplicity and
fast builds. Rust pays at compile time and in learning effort, and buys runtime efficiency *together with* safety. C and
C++ pay in incidents.

> **Why not C++?** Modern C++ (smart pointers, RAII everywhere, the Core Guidelines, sanitizers, static analyzers) is far
> safer than the C++ of 2005, and C++ owns enormous ecosystems (games, HPC, trading, browsers). The difference is that
> in C++ safety is a **practice** and in Rust it's a **default the compiler enforces**. Practices erode under deadlines
> and team turnover. Enforced defaults don't. If your team and codebase are C++, the right question is usually "where
> do new components go?", not "rewrite?".

> **Why not Go?** For many networked services Go is the better business choice: a small language, fast builds, a
> good-enough runtime, and a team productive in weeks. Rust earns its extra cost when you need what Go's runtime can't
> give you: no GC (for tail latency or memory ceilings), data-race freedom enforced at compile time, zero-overhead
> abstractions in CPU-bound code, or embedding in other runtimes. Chapter 1.4 turns this into a decision framework.

### 8. Java comparison

**Where Java's bet beats Rust's.** Be honest about this, because your Java experience is an asset here and you shouldn't
throw it away:

- **The JIT sees the real workload.** Speculative inlining, devirtualization, and escape analysis are driven by live
  profiles. For large, polymorphic business codebases this is a genuine advantage.
- **GC makes graphs free.** Domain models with bidirectional associations, caches of shared objects, and observer webs
  cost nothing extra to express. In Rust they need design (Chapter 1.4).
- **Observability is unmatched.** JFR, async-profiler, heap dumps, and live agents let you inspect a running production
  JVM in ways that native binaries only partly match.
- **Virtual threads (JDK 21+)** let blocking-style code scale to very large numbers of concurrent tasks without async
  coloring. JDK 24 removed most of the pinning around `synchronized`.
- **The ecosystem and the hiring pool** are enormous.

> **Analogy limit: tuning the runtime vs designing the data.** In Java, performance engineering mostly means **tuning the
> runtime**: heap sizes, collector choice, JIT flags, allocation-rate reduction so the GC runs less. In Rust there's no
> runtime to tune. Performance engineering means **designing the data and its ownership**: layout, where allocation
> happens, who owns what, and when things are freed. A Java engineer who looks for Rust's `-Xmx` won't find one. The
> knobs moved into the code. That's the biggest mental shift in this book.

### 9. Production scenario

Meridian doesn't need one language. It needs a **language per component**, chosen by the shape of the workload, the
team, and the ecosystem:

| Meridian component | Workload shape | Leading candidates | Why |
|---|---|---|---|
| **API gateway** (TLS termination, routing, rate limits; ~400K req/s peak) | CPU-bound per request, huge fan-in, tail latency matters, deployed on many nodes | Rust or Go (Java + Netty viable) | Per-request CPU and memory multiply by fleet size; no-GC tail latency |
| **Ledger service** (double-entry accounting, ~3K TPS) | Complex domain, database-bound, correctness enforced by transactions | Java/Kotlin | Domain modeling and team velocity dominate; the database is the bottleneck, not the language |
| **Fraud feature extraction** (50K scores/s, p99 < 5 ms) | CPU-bound hot path inside a Java service | A Rust library called from Java (FFM API), or allocation-conscious Java | Optimize the hot 5% without rewriting the other 95% |
| **Market-data fan-out** (from Chapter 1.1) | Latency-critical, memory-unsafe today | Rust | A memory-safety incident history plus microsecond budgets |
| **Kubernetes operators and deploy tooling** | Glue code against Kubernetes APIs | Go | The ecosystem (client-go, controller-runtime) decides it |

Notice that the language decision followed from the **workload shape**, not from anyone's taste. That's the habit Part I
is trying to build.

### 10. Failure scenario

**The same bug in four languages: a shared map mutated from two threads.**

| Language | What happens |
|---|---|
| **C++** (`std::unordered_map`) | Concurrent inserts corrupt bucket arrays or allocator state. UB: crashes, lost entries, or silent corruption, reported far from the cause. |
| **Java ≤ 7** (`HashMap`) | A well-known failure mode: two threads resizing at once could create a **cycle** in a bucket's linked list, so a later `get()` spins forever at 100% CPU. There was no exception and no corruption, just a hung thread pool in production. JDK 8 rewrote resizing so that particular cycle can't form, but `HashMap` still isn't safe for concurrent writers. You can lose updates, and "it didn't hang" isn't "it's correct." |
| **Go** (`map`) | The runtime detects some concurrent map misuse and kills the process with `fatal error: concurrent map writes`. This is a runtime check, it isn't guaranteed to fire, and `recover()` can't catch it. |
| **Rust** (`HashMap`) | Doesn't compile. Two threads can't both hold `&mut` to the map (the same E0499 you'll see in Chapter 1.3), and moving it into a spawned thread needs ownership that can't be shared. You must pick `Mutex<HashMap>`, `RwLock<HashMap>`, a sharded or concurrent map, or single-owner message passing. |

There's a spectrum here. Java makes the bug *safe but wrong*, Go makes it *loud but late*, C++ makes it *silent*, and
Rust makes it *impossible to write*. None of the runtime options were free. Each needed a production incident to find.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part I).*

1. Before 1.0, Rust had a garbage collector, green threads, and segmented stacks. Why remove them? What did Rust gain,
   and what did it lose?
2. Go and Rust both compile ahead of time to native code. Why does every Go binary contain a scheduler and a GC while a
   Rust binary doesn't? What does each design buy?
3. Explain how a JIT can outperform ahead-of-time compiled code on the same algorithm. What can an AOT compiler, or a
   language design, do to close the gap?
4. Draw the memory layout of `ArrayList<Point>` in Java and `Vec<Point>` in Rust. When does the difference matter for
   performance, and when is it noise?
5. Compare stackful coroutines (goroutines, Java virtual threads) with Rust's stackless futures on memory per task,
   what happens on a blocking call, and what happens on deep recursion.
6. Explain how a data race in Go can break memory safety. Use an interface value in your answer.
7. Steelman this claim, then evaluate it: "Modern C++ with smart pointers, RAII, and sanitizers is as safe as Rust in
   practice."
8. Rust claims C++'s zero-overhead principle. Name a place where Rust deliberately pays a runtime cost C++ doesn't, and
   justify it.

### 12. Exercises

- **Beginner.** Make a five-row table (C, C++, Java, Go, Rust) for "how is absence represented?" and "what happens if
  code forgets to check?". Include `Option<&T>`'s size guarantee in the Rust row.
- **Intermediate.** For the `make()` example, write both Rust fixes (return by value, return `Box<i32>`). For each,
  say where the `42` lives and who frees it. Then explain why Go's automatic heap promotion is a *performance*
  concern even though it's a *safety* win.
- **Advanced.** An order struct has `id: u64`, `price: f64`, `qty: u32`, `side: u8`. Compute the memory for **10
  million orders** as a Rust `Vec<Order>` and as a Java `ArrayList<Order>` (compressed oops, 12-byte headers, 8-byte
  alignment). Show the padding. Then redesign the Java side as struct-of-arrays and recompute. What did you give up?
- **Systems.** Predict how many OS threads each of these has when idle: (a) a Java "hello world" that sleeps for 60 s;
  (b) the same in Go; (c) the same in Rust. Then measure on Linux or WSL with `ls /proc/<pid>/task | wc -l` (or Process
  Explorer on Windows). Explain every thread you can identify.
- **Architecture.** Extend the Meridian component table with two components from your own work. For each, write down
  the workload shape first and the language second. Did the shape decide it, or something else (team, ecosystem,
  existing code)? Both are legitimate. Say which one it was.

### 13. Debugging exercise

`largest` compiles on its own. This caller doesn't (full file: `listings/part-01/ch02-03-largest-dangling.rs`):

```rust,compile_fail
// `largest` as defined in §3
fn main() {
    let top;
    {
        let scores = vec![3, 9, 4];
        top = largest(&scores);
    }
    println!("{top:?}");
}
```

1. Explain error E0597 as an ownership argument: who owns the `9`, when is it freed, and what does `top` point to at
   the `println!`?
2. Give two fixes. One should keep `scores` alive longer. The other should change what `top` *is* so it no longer
   borrows from `scores`.
3. Your second fix probably needed `T: Clone` or `T: Copy`. When is that a good trade and when is it a bad one? Think
   about `T = u64`, `T = String`, and `T = [u8; 4096]`.

### 14. Design exercise

**Meridian's new risk-limits service.** It keeps per-account exposure limits in memory, applies about 100,000
limit-check-and-update operations per second, and must answer with p99 < 1 ms, because every order waits on it. The team
is six experienced Java engineers, nobody knows Rust, and there's a six-month deadline. The service must never approve an
order that breaches a limit, even under concurrent updates.

Choose Java, Go, or Rust. Write a one-page ADR covering: the latency budget and where it goes, how you'll guarantee the
no-breach invariant under concurrency (compare the mechanisms each language gives you), memory footprint, team ramp-up
risk, and operational tooling. Include the conditions under which you'd reverse the decision.

A strong answer might choose *any* of the three. What matters is whether the reasoning holds up.
