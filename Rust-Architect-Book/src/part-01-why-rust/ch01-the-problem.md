# Chapter 1.1 — The Problem: Who Frees This Memory?

> **Where this sits:** Part I · Why Rust Exists · chapter 1 of 4
> **Prerequisites:** the preface.
> **After this chapter you can:** classify memory-safety failures precisely, explain why undefined behavior is worse than
> a crash, and state the one question that every memory-management design answers.

---

## Pass 1 · User level — *What goes wrong, and how does it look in code?*

### 1. Problem

Systems software (databases, proxies, kernels, browsers, language runtimes, storage engines) has two requirements that
pull against each other:

1. **Control.** You decide where data lives, how it's laid out, when it's allocated and freed, and which thread touches
   it. Without that control you can't align a B-tree node to a disk page, keep a hot loop's working set in L1 cache, or
   hold a p99.9 latency target measured in microseconds.
2. **Correctness under adversarial input.** Systems code parses bytes from the network, the disk, and other processes,
   and any of those bytes can be hostile.

For about forty years you mostly had to pick one. **C and C++** give you control and leave correctness to discipline.
**Java, C#, Go**, and similar languages give you memory safety by putting a runtime (a garbage collector, bounds checks,
a managed heap) between you and the machine, and they take some control away in exchange.

Discipline didn't scale. Three public data points, each a vendor reporting on its own codebase (treat them as
directional evidence, not universal constants):

- **Microsoft reported (2019)** that about 70% of the vulnerabilities it assigns a CVE each year are memory-safety issues.
- **The Chromium project reported (2020)** that about 70% of its high-severity security bugs are memory-safety bugs.
- **Google reported (2024)** that memory-safety vulnerabilities fell from 76% of Android's total in 2019 to 24% in 2024,
  mainly by writing *new* code in memory-safe languages (Rust, Kotlin, Java) and without rewriting old code.

The last one matters most architecturally, and Chapter 1.4 comes back to it. For now, the problem statement is:

> **Can a language give C-level control and still make whole classes of memory and concurrency bugs impossible,
> without a garbage collector?**

Rust is a bet that the answer is "mostly yes, if you're willing to pay at compile time and in learning effort." Part I
evaluates that bet honestly.

### 2. Mental model

Every piece of heap memory goes through the same lifecycle:

```text
   allocate ───► initialize ───► use (read / write / share) ───► free
       │              │                    │                        │
   who asks?    before any read?    who else can see it?      exactly once?
                                    is anyone writing?        only after the
                                                              last use?
```

**Every memory-safety bug violates this lifecycle.** The classes fall into five families:

```text
                              MEMORY-SAFETY FAILURES
                                       │
    ┌──────────────┬───────────────────┼────────────────────┬──────────────────┐
 SPATIAL        TEMPORAL         INITIALIZATION        CONCURRENCY           TYPE
 "where"         "when"            "before"          "who, at once"        "as what"
    │               │                   │                    │                  │
 out-of-bounds   use-after-free     read of            data race          type confusion
 read / write    double free        uninitialized      (unsynchronized    (bytes read as
 buffer          dangling pointer   memory             conflicting         the wrong type)
 overflow        iterator                              accesses)
                 invalidation
```

Heartbleed (OpenSSL, 2014) was a **spatial** bug: a length field supplied by an attacker drove a copy past the end of a
buffer and leaked process memory. Most browser exploits of the last decade start with a **temporal** bug, a
use-after-free. **Data races** sit in the same taxonomy, because in C, C++, and (for multi-word values) Go, a race can
corrupt memory.

Behind all five sits one question. Every language's memory-management design is an answer to it:

> **Who is responsible for freeing this memory, and how do we know nobody is still using it when we do?**

| Answer | Who decides when to free | How "nobody still uses it" is established | Examples |
|---|---|---|---|
| **Manual** | The programmer, explicitly | The programmer's reasoning. Nothing checks it. | C `malloc` / `free` |
| **Library-assisted RAII** | Scope exit or refcount, via destructors | Partly types, mostly convention. Not enforced. | C++ `unique_ptr`, `shared_ptr` |
| **Tracing GC** | A runtime, periodically | The runtime proves unreachability by tracing from roots | Java, Go, C#, JavaScript |
| **Reference counting** | The last reference's drop | Counter reaches zero (cycles need extra help) | Swift ARC, CPython, Rust `Rc`/`Arc` (opt-in) |
| **Static ownership + borrowing** | The owner's scope; the compiler inserts the free | **The compiler proves** no reference outlives its owner | Rust |

Rust didn't invent a sixth answer out of nothing. It took C++'s RAII, made moves destructive, and added a compile-time
checker that turns "by convention" into "by proof." Chapter 1.3 takes that apart.

### 3. Rust code

Here is the smallest program that shows the whole problem. First in C++:

```cpp
#include <iostream>
#include <vector>

int main() {
    std::vector<int> v = {1, 2, 3};
    int& first = v[0];            // a reference into v's heap buffer
    v.push_back(4);               // may reallocate: elements move, the old buffer is freed
    std::cout << first << '\n';   // if it did, this reads freed memory: undefined behavior
}
```

It compiles, typically without a warning at default settings. It might print `1`, might print garbage, or might do
something stranger. The language doesn't say which.

The same program in Rust:

```rust,compile_fail
fn main() {
    let mut v = vec![1, 2, 3];
    let first = &v[0];
    v.push(4);
    println!("first = {first}");
}
```

doesn't compile. This is the real output of rustc 1.98.1:

```text
error[E0502]: cannot borrow `v` as mutable because it is also borrowed as immutable
 --> src/main.rs:5:5
  |
4 |     let first = &v[0];
  |                  - immutable borrow occurs here
5 |     v.push(4);
  |     ^^^^^^^^^ mutable borrow occurs here
6 |     println!("first = {first}");
  |                        ----- immutable borrow later used here
```

The error names three program points: where the shared borrow starts, where a conflicting mutable borrow is requested,
and where the shared borrow is still needed. That's a proof sketch. The compiler is showing you an ordering of events
that would be unsafe. Part IV teaches you to read every borrow-checker error this way.

And in Java:

```java
List<Integer> list = new ArrayList<>(List.of(1, 2, 3));
for (Integer x : list) {
    if (x == 2) list.add(4);      // the iterator's next step throws ConcurrentModificationException
}
```

Java can't dangle. The garbage collector keeps every reachable object alive, so a stale reference is never a reference
to freed memory. What Java *does* detect is the logical problem, structural modification during iteration, at run time
through a modification counter (`modCount`). The `ArrayList` documentation says this detection works "on a best-effort
basis." It means that literally:

```java
List<Integer> list = new ArrayList<>(List.of(1, 2, 3));
for (Integer x : list) {
    if (x == 2) list.remove(Integer.valueOf(2));   // no exception, and 3 is silently never visited
}
```

After removing the second-to-last element, the iterator's `hasNext()` compares `cursor != size`, gets `2 != 2`, and the
loop ends quietly with an element never processed.

Three languages, three answers to the same bug:

| | C++ | Java | Rust |
|---|---|---|---|
| When is the bug detected? | Maybe never | At run time, best-effort | At compile time, always |
| What happens? | Undefined behavior | Exception (usually) or a silent skip | Program rejected |
| What it costs | Nothing at run time; unbounded cost in incidents | A counter check per iteration step, plus GC | Compile-time analysis; some *correct* programs are rejected too |

The last cell matters. Rust's rule is conservative, and you'll see exactly how conservative in a moment. Here are three
ways to satisfy it (all verified):

```rust
fn main() {
    // Fix 1: copy the value out. `i32` is `Copy`, so no borrow survives.
    let mut v = vec![1, 2, 3];
    let first = v[0];
    v.push(4);
    println!("fix 1: first = {first}");

    // Fix 2: remember a position, not an address.
    let mut v = vec![1, 2, 3];
    let first_idx = 0;
    v.push(4);
    println!("fix 2: first = {}", v[first_idx]);

    // Fix 3: finish using the borrow before mutating.
    let mut v = vec![1, 2, 3];
    let first = &v[0];
    println!("fix 3: first = {first}");
    v.push(4);
    println!("fix 3: len = {}", v.len());
}
```

```text
fix 1: first = 1
fix 2: first = 1
fix 3: first = 1
fix 3: len = 4
```

Each fix is a different *design*. Fix 1 copies data. Fix 2 swaps an address for a stable identifier, the most important
pattern in systems Rust (arenas, slabs, entity IDs, and file offsets are all this idea). Fix 3 reorders time. The
compiler didn't choose one for you. It refused to let you avoid choosing.

---

## Pass 2 · Systems level — *What does the compiler know, and what happens in memory?*

### 4. Under the hood

**How did rustc reject that program without knowing that `push` reallocates?**

It didn't need to know. The borrow checker never looks inside `Vec::push`. It looks at the function's **signature**:

```rust,ignore
pub fn push(&mut self, value: T)
```

`&mut self` means "for the duration of this call, I need **exclusive** access to the vector." Meanwhile `first` is a
**shared** borrow of `v` that must stay valid until the `println!`. Exclusive access while a shared borrow is live
breaks the core rule, **aliasing XOR mutation**: at any moment a value can have many readers or one writer, never both.
The rule doesn't care *why* `push` wants exclusivity. Reallocation is one reason, but any mutation at all would be
enough.

You can see this directly. Give the vector plenty of spare capacity so that no reallocation could happen, and the
program is still rejected:

```rust,compile_fail
fn main() {
    let mut v = Vec::with_capacity(16);
    v.extend([1, 2, 3]);
    let first = &v[0];
    v.push(4); // capacity is 16, so no reallocation will happen...
    println!("first = {first}"); // ...but the borrow checker still rejects this
}
```

```text
error[E0502]: cannot borrow `v` as mutable because it is also borrowed as immutable
```

This program happens to be memory-safe at run time, and Rust rejects it anyway. That's a design decision, and it's worth
understanding as an architect:

- **Checking is local and signature-based.** [LANG] Each function body is checked against the *signatures* of what it
  calls. The signature is the contract. [RUSTC] The checker is intraprocedural: it analyzes one function at a time and
  never does whole-program analysis.
- **That locality is what makes it scale.** `Vec`'s implementation can change (a different growth policy, a different
  allocator) without invalidating the safety proof of a single caller anywhere. The same modularity is what lets
  million-line Rust codebases and large dependency graphs compose safely.
- **The price is false positives.** A checker that reasons only from signatures has to be conservative. Some correct
  programs get rejected, and you restructure them (indices, scoping, copying) or, rarely, reach for `unsafe` and carry
  the proof yourself.

> **What actually happens?** Where does the error come from in the pipeline? [RUSTC] Borrow checking runs on **MIR**
> (Mid-level IR), a simplified control-flow-graph form of your function, after type checking and before code generation.
> Since the 2018 edition it uses **non-lexical lifetimes (NLL)**: a borrow lasts until its *last use*, not until the end
> of its lexical scope. That's why Fix 3 compiles. `first` is dead before `push`. [VERSION] A more precise successor
> formulation, **Polonius**, has been in development for years to accept more correct programs. Check its status before
> relying on it. Parts IV and XVIII cover all of this.

**What does "undefined behavior" actually permit?** In C and C++, the standard places *no requirements* on a program
that executes UB. Compilers use that freedom aggressively, because it lets the optimizer assume UB never happens:

```c
int will_overflow(int x) {
    return x + 1 < x;    // signed overflow is UB, so an optimizing compiler may fold this to `return 0;`
}
```

GCC and Clang at `-O2` typically compile this to a function that always returns `0`: if overflow "can't happen,"
`x + 1 < x` is always false. A real case of the same logic hit the Linux kernel (CVE-2009-1897). A pointer was
dereferenced *before* it was checked for null. From the dereference the compiler inferred that the pointer couldn't be
null, so it deleted the null check. The source contained a safety check and the binary didn't.

So UB isn't "the program crashes." It's "the program you wrote is not the program that runs."

### 5. Memory

What the C++ program does in memory, with growth numbers that are **implementation-defined**:

```text
BEFORE push_back  (len 3, cap 3)

 stack                               heap
 ┌──────────────┐                    ┌─────┬─────┬─────┐
 │ v.begin  ────┼──────────────────► │  1  │  2  │  3  │   block A (12 bytes of payload)
 │ v.end        │                    └─────┴─────┴─────┘
 │ v.cap_end    │                       ▲
 ├──────────────┤                       │
 │ first  ──────┼───────────────────────┘
 └──────────────┘

AFTER push_back   (len 4; libstdc++ and libc++ double to cap 6, MSVC's STL grows 1.5x to cap 4)

 stack                               heap
 ┌──────────────┐                    ┌─────┬─────┬─────┬─────┬─────┬─────┐
 │ v.begin  ────┼──────────────────► │  1  │  2  │  3  │  4  │     │     │   block B (new)
 │ ...          │                    └─────┴─────┴─────┴─────┴─────┴─────┘
 ├──────────────┤                    ┌ ─ ─ ┬ ─ ─ ┬ ─ ─ ┐
 │ first  ──────┼──────────────────► │ ??  │     │     │   block A: FREED. Back in the allocator's
 └──────────────┘                    └ ─ ─ ┴ ─ ─ ┴ ─ ─ ┘   free list; may already hold someone else's data
```

Rust's `Vec` has the **same** physical layout (pointer, capacity, length) and the **same** reallocate-and-copy growth.
[LIB] Today's std implementation also roughly doubles. The std docs deliberately leave the growth strategy unspecified,
so it isn't a [LANG] guarantee. So:

> **Rust's `Vec` is not safer at run time than `std::vector`.** The safety comes from the fact that the program which
> would observe the stale pointer *cannot be written* in safe Rust. The runtime data structure is identical. The set of
> expressible programs is different.

That idea recurs throughout the book. Rust's safety mostly lives in the **type system**, not in runtime machinery.

### 6. CPU / OS

**Why does a use-after-free so rarely crash on the spot?**

- [OS] The MMU faults only on **unmapped or protected pages**. `free()` almost never unmaps anything. Memory allocators
  (glibc's malloc, jemalloc, mimalloc, the Windows heap) keep freed chunks in user-space free lists for fast reuse. The
  page stays mapped, so the load through `first` succeeds and returns whatever those bytes hold *now*.
- [LIB] What they hold now may be allocator metadata. glibc's per-thread cache (tcache), for example, stores a free-list
  link in the first bytes of a freed small chunk. So `first` might read part of the allocator's own pointer. Or the
  chunk may already be recycled into a different object, and you'd be reading someone else's data as an `int`.
- **Writing** through a dangling pointer is worse. It corrupts allocator metadata or a live object, and the crash (if one
  ever comes) happens later, somewhere else, usually inside `malloc`/`free` with an error like
  `free(): invalid pointer` or `malloc(): corrupted top size`.
- This is also the raw material of exploitation. An attacker who can influence *what* gets allocated into the freed slot
  can get the program to treat attacker-shaped bytes as, say, an object with a vtable pointer. That's type confusion,
  and from there it's a short step to control-flow hijack.

**Detection tools, and what they can and can't do:**

| Tool | How it works | Limits |
|---|---|---|
| AddressSanitizer (ASan) | Compiler instrumentation + shadow memory + a quarantine for freed chunks | Only catches bugs on **executed** paths; roughly 2x CPU and several times the memory, so test builds, not production |
| Valgrind (memcheck) | Dynamic binary instrumentation | Much slower still; test only |
| ARM MTE (Memory Tagging) | Hardware tags on pointers and 16-byte memory granules | **Probabilistic** (4-bit tags) and needs supporting hardware |
| CHERI capabilities | Hardware-enforced bounded pointers | Research/prototype hardware; needs recompilation, sometimes code changes |

All of these are **mitigations**. They reduce the odds that a bug turns into an incident, but none of them *prove* the
bug can't exist. Rust's compile-time check is a proof, over the programs it accepts.

**What about data races at the CPU level?** `counter += 1` compiles to load, add, store. Two cores interleaving those
three steps lose updates. It gets worse. In C and C++ a data race is UB, so the compiler may assume a non-atomic variable
isn't changed by another thread. It can keep the value in a register, or hoist a load out of a loop, which turns
`while (!done) {}` into an infinite loop. On top of that, CPU store buffers let other cores see writes in a different
order. Part XIV covers this properly. For now: **at the hardware level, "shared mutable memory" isn't a simple thing,
and languages that allow unsynchronized access inherit all of that complexity.**

---

## Pass 3 · Architect level — *What are the options, and what does each one cost?*

### 7. Trade-offs

Five strategies for temporal safety, compared as an architect would compare them (qualitative; later Parts measure):

| Dimension | Manual (C) | RAII + smart pointers (C++) | Tracing GC (Java, Go) | Ref counting (Swift; Rust `Rc`/`Arc`) | Ownership + borrowing (Rust) |
|---|---|---|---|---|---|
| **Runtime CPU for lifecycle** | None | Near none (`unique_ptr`); atomic inc/dec for `shared_ptr` | GC threads, write/read barriers | Inc/dec on every copy; atomic if shared across threads | None |
| **Memory overhead** | None | Control block for `shared_ptr` | **Headroom:** heap must exceed the live set | Counter per object | None |
| **Latency predictability** | High | High | Pauses (now short), allocation stalls, GC CPU competing with yours | Cascading frees can spike | High |
| **Deterministic release** (files, sockets, locks) | Manual | Yes | No; memory is reclaimed "eventually" | Yes | Yes |
| **Cyclic graphs** | Manual | Leak with `shared_ptr` unless `weak_ptr` | **Free**, handled naturally | Leak unless weak refs | Need `Rc`+`Weak`, arenas, or indices |
| **Safety guarantee** | None | None (by convention) | Temporal + spatial | Temporal (for counted objects) | Temporal + spatial + data-race-free (safe code) |
| **Developer effort** | High, invisible until the incident | Medium | **Lowest** | Low–medium | Front-loaded: high while learning, lower later |
| **Where it breaks down** | Scale of team and code | Discipline at scale | Memory-bound or latency-critical workloads | Contended shared counters, cycles | Graph-heavy domains, fast prototyping |

About **headroom**: an influential study (Hertz & Berger, OOPSLA 2005) found that a garbage collector needed roughly
**five times** the memory of the live data to match explicit memory management's performance. With three times it ran
about 17% slower, and with only twice the memory about 70% slower. Collectors have improved a lot since 2005, and the
exact numbers don't carry over to modern JVMs. The *shape* of the trade-off still holds, though: **a tracing GC trades
memory for CPU.** Give it less headroom and it runs more often.

> **Why not just be careful?** Because being careful doesn't compose. One engineer can hold a C++ module's lifetime
> rules in their head. A 200-engineer organization with ten years of churn can't. The 70% figures above come from some of
> the best-resourced, most careful C++ teams in the world, with fuzzing, sanitizers, and code review. At that scale, a
> rule is only reliable if a machine checks it.

> **Why not run ASan in production?** Several times the memory, around 2x CPU, and it still only catches bugs on the
> paths that actually execute. It's excellent for finding bugs in C and C++. It doesn't give you a guarantee.

### 8. Java comparison

Java answers the lifecycle question with four mechanisms: a **tracing GC** (temporal safety), **bounds checks**
(spatial safety), **no pointer arithmetic**, and **definite assignment** plus default field values (initialization
safety). That combination is very effective, and it's why Java took over enterprise backends.

The concurrency row is where it gets interesting:

| | Data race possible? | Can a data race corrupt memory? |
|---|---|---|
| C / C++ | Yes | Yes. It's UB. |
| Go | Yes | **Yes, for multi-word values** (interfaces, slices, strings): a torn read can pair a type pointer with the wrong data |
| Java | Yes | **No.** The Java Memory Model guarantees reference reads are never torn. You can see stale or reordered values, and non-`volatile` `long`/`double` writes may be split in two (JLS §17.7), but you can't forge a pointer. |
| Safe Rust | **No**, rejected at compile time | n/a |

So Java gives you **memory safety without data-race freedom**. Your racy Java program won't corrupt the heap, but it can
still compute wrong answers. Safe Rust rules out the race itself.

> **Analogy limit.** "Rust ownership is like Java's GC, but at compile time" is useful up to exactly this point:
> both prevent use-after-free. After that they diverge in both directions.
> - **GC solves problems ownership doesn't:** arbitrary object graphs, cycles, sharing without ceremony. In Java, a
>   doubly linked list or an observer graph is trivial. In Rust it takes deliberate design (Chapter 1.4, Part III).
> - **Ownership solves problems GC doesn't.** GC prevents *use-after-free*, not *use-after-invalidate*. In Java the
>   object stays alive, but it can be logically stale: an iterator over a modified list, a `subList` view after the
>   backing list changed, a connection object you still hold after returning it to the pool. The
>   `ConcurrentModificationException` example is exactly this class of bug. Rust's aliasing rule prevents the logical
>   version too, because it's about *access*, not *liveness*.

### 9. Production scenario

**Meridian's market-data fan-out** is a C++ service. It keeps a `std::vector<Subscriber>` and, during a broadcast, holds
`Subscriber&` references while it serializes and sends. New subscribers can join mid-broadcast, on a control thread. In
every test, the vector's capacity was big enough that `push_back` never reallocated. In production, a morning traffic
burst coincided with a wave of reconnects. The vector reallocated mid-broadcast, the references dangled, and the service
started sending fragments of *other subscribers' buffers*. That's a data leak, not just a crash.

Port that design to Rust and it doesn't compile: the broadcast holds shared borrows while the join path wants `&mut`.
(Across threads it fails even earlier, because the vector can't be shared mutably without synchronization at all.) The
compiler forces an explicit design choice, and each option is a legitimate architecture:

| Option | Mechanism | Cost profile |
|---|---|---|
| Stable IDs + slab/arena | `Vec<Subscriber>` + generation-checked indices | Cache-friendly, no refcounts; stale IDs are *detected*, not dereferenced |
| Shared ownership | `Vec<Arc<Subscriber>>` | Subscribers outlive the vector safely; atomic refcount traffic on every clone |
| Snapshot / copy-on-write | Broadcast iterates an immutable snapshot; joins publish a new one | Readers never block; writers pay a copy |
| Two-phase update | Queue joins, apply between broadcasts | Simplest; adds join latency of up to one broadcast period |

Chapter 1.1's design exercise asks you to pick one and defend it.

### 10. Failure scenario

The incident behind the scenario above, as it tends to play out:

```text
Day 0   Deploy. Everything fine. (Capacity never exceeded in tests or canary.)
Day 9   Market-open burst + reconnect storm. First reallocation during a broadcast.
        Symptom: 3 subscribers receive malformed frames; 1 receives bytes from another customer's stream.
Day 9   Crash: SIGABRT in free(): "free(): invalid pointer". Core dump stack is in the logging library.
Day 10  Can't reproduce under load test. Suspect the logging library; upgrade it. No change.
Day 14  Second crash, different stack (inside the JSON serializer).
Day 17  ASan-instrumented canary catches "heap-use-after-free" with the allocation and free stacks.
        Root cause found: 17 days, two unrelated suspects, one data exposure.
```

The lesson is **cause–symptom distance**. With memory corruption, the line that goes wrong is rarely the line that
crashes, and the stack trace points at a victim. Compare the failure ladder:

```text
 BEST  ┌─ compile-time error            cause and detection in the same place, before deploy
   │   ├─ deterministic runtime check   panic/exception at the faulting line (Rust index OOB, Java AIOOBE)
   │   ├─ best-effort runtime check     usually detected, sometimes silently not (Java CME)
   │   ├─ sanitizer in test             detected only if the buggy path ran under instrumentation
 WORST └─ undefined behavior            anything, anywhere, later, possibly never
```

An architect's version of the principle: **push failures left and make them local.** Rust's value proposition is
largely about moving whole bug classes to the top rung.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part I). Write yours first.*

1. Why is a use-after-free usually harder to diagnose than a null-pointer dereference? Answer in terms of the OS, the
   allocator, and cause–symptom distance.
2. Rust rejected the `push` example even when the vector had spare capacity. Is that a flaw? What would it cost, in
   compiler design and in the stability of library APIs, to make the checker "smarter"?
3. Java programs can't have a use-after-free. Can they have its *logical* equivalent? Give two examples from real
   backend code.
4. What exactly does undefined behavior permit a C++ compiler to do? Why do compiler engineers *want* UB to exist?
5. A colleague says: "We run ASan and fuzzing in CI, so our C++ is memory-safe." Evaluate that claim precisely.
6. Why is a data race a memory-safety problem in C++ and Go, but "only" a correctness problem in Java?
7. Your service parses untrusted protobufs at 200K messages/second. Which classes in the taxonomy matter most, and
   which mitigations would you layer, in which language?
8. A tracing GC solves temporal safety. What does it cost, and under which workload shapes do those costs dominate?

### 12. Exercises

- **Beginner.** Classify each bug into the taxonomy (spatial / temporal / initialization / concurrency / type):
  (a) Heartbleed; (b) returning a pointer to a local variable; (c) `memcpy` with a length taken from a packet header;
  (d) two threads incrementing a shared `int`; (e) reading a struct field before any constructor ran; (f) casting a
  `Base*` to the wrong `Derived*`; (g) calling `free` twice on an error path; (h) iterating a `std::map` while erasing
  from it.
- **Intermediate.** Take the three fixes from §3 and apply them to a `Vec<Order>`, where `Order` is a 200-byte struct
  that isn't `Copy`. Which fixes still work unchanged? Which one would you pick, and what does each cost in memory and
  CPU?
- **Advanced.** Read the "Guarantees" section of the `std::vec::Vec` documentation. List three properties of `Vec`'s
  memory behavior that are **guaranteed** and three that are **implementation details**. Why would the library team keep
  the growth factor unspecified?
- **Systems.** On Linux (WSL works) or Compiler Explorer (godbolt.org), compile the C++ program from §3 with
  `-fsanitize=address -g` and run it. Explain each section of the report: *heap-use-after-free*, *freed by thread T0
  here*, *previously allocated by*. Then compile *without* ASan at `-O2` and describe what you observe and why.
- **Architecture.** List the five most serious production incidents your team has had in the last two years. Classify
  each into the taxonomy, or mark it "not a memory-safety issue." For each, say whether Rust would have prevented it,
  made it louder, or made no difference. (Most teams find the answer is "made no difference" for more incidents than
  they expected. That's the honest baseline for Chapter 1.4.)

### 13. Debugging exercise

This code is rejected. **Before** running it: predict the error code and the three program points the error will cite.
Then explain the error as an ownership argument in plain English, and give **three different fixes** with the
trade-off of each.

```rust,compile_fail
#[derive(Debug)]
struct User {
    id: u64,
    name: String,
}

struct Registry {
    users: Vec<User>,
}

impl Registry {
    fn find(&self, id: u64) -> Option<&User> {
        self.users.iter().find(|u| u.id == id)
    }

    fn add(&mut self, user: User) {
        self.users.push(user);
    }
}

fn main() {
    let mut registry = Registry {
        users: vec![User { id: 1, name: "ada".into() }],
    };

    let ada = registry.find(1).expect("ada exists");
    registry.add(User { id: 2, name: format!("{}'s colleague", ada.name) });
    println!("found {:?}", ada);
}
```

Hint for the discussion: `find` returns a reference *into* `registry`. What does the signature `fn find(&self, ...) -> Option<&User>`
tell the compiler about how long that reference may live?

### 14. Design exercise

**Meridian's subscriber registry.** Design the registry for the fan-out service from §9 with these requirements:

- 50,000 subscribers per process; joins and leaves at up to 2,000/second during reconnect storms.
- Broadcasts every 5 ms to all subscribers; the p99 broadcast must finish in under 2 ms.
- A subscriber that leaves must never receive another frame after its leave is acknowledged.
- The service runs on 16 cores.

Pick one of: stable IDs in a slab, `Vec<Arc<Subscriber>>`, copy-on-write snapshots, two-phase update, or a design of your
own. Write a half-page ADR covering memory, CPU, latency, contention, failure modes, and operational complexity. State
what would make you change your mind (for example, "if joins exceeded 20,000/second, I'd switch to...").

There's no single right answer. Reviewers judge the quality of the justification.
