# Chapter 1.4 — The Honest Cost

> **Where this sits:** Part I · Why Rust Exists · chapter 4 of 4
> **Prerequisites:** Chapters 1.1–1.3.
> **After this chapter you can:** list what Rust actually costs, explain *why* each cost exists, do the arithmetic that
> tells you whether runtime efficiency matters for a given workload, and argue for **or against** Rust in a design
> review.

---

## Pass 1 · User level — *Where does Rust hurt?*

### 1. Problem

Chapter 1.3 was the sales pitch. This chapter is the invoice.

Rust's guarantees are paid for. The costs don't disappear. They move: from production into development, from the
runtime into the compiler, from the incident review into the code review. Whether that trade is a good one depends
entirely on **where your costs are today**. For a latency-critical proxy fleet it can be a large win. For a
database-bound CRUD service written by a Java team on a deadline it can be a loss. An architect has to be able to tell
which situation they're in, and has to be willing to say "not Rust" when that's the answer.

### 2. Mental model

**Rust is a front-loaded language.** It charges more up front and less over time:

```text
                 LEARN            BUILD                RUN                  OPERATE (years)
             ─────────────  ─────────────────  ──────────────────  ────────────────────────────────
 Java / Go    low            low (fast builds,   higher (GC, JIT     memory/runtime tuning; some bug
              (weeks)        forgiving design)   warm-up, headroom)  classes surface as incidents

 Rust         HIGH           higher (slow        low (no GC, lean    fewer memory/concurrency incidents;
              (months to     builds, design      footprint)          refactors checked by the compiler
              fluency)       effort up front)
```

The trade pays off when the right-hand columns are big: **long-lived code, large scale, strict latency or memory
limits, safety-critical parsing, or embedding in other systems.** It doesn't pay off when they're small: prototypes,
short-lived tools, and services where the database and the network account for 95% of the latency.

The six costs, which the rest of the chapter explains:

```text
 1. Learning curve          ownership, borrowing, lifetimes, traits: new mental models, not new syntax
 2. Compile times           monomorphization + LLVM + crate-level compilation
 3. Graph-shaped data       ownership is a tree; graphs need Rc/Weak, arenas, or indices
 4. Lifetime propagation    borrowing in a struct spreads a lifetime parameter to every holder
 5. Async complexity        function coloring, Pin, cancellation-by-drop, runtime choice
 6. Ecosystem & people      thinner enterprise libraries, fewer runtime introspection tools, smaller hiring pool
```

### 3. Rust code

**Cost 3: graph-shaped data.** In Java, a tree with parent links is five lines and needs no thought:

```java
class Node {
    String name;
    Node parent;                            // back-edge: the GC doesn't care about cycles
    List<Node> children = new ArrayList<>();
}
```

In Rust, `parent: &Node` won't work, because a child can't borrow its parent while the parent owns (and may mutate)
the child. You have two idiomatic options. The first is **shared ownership with weak back-edges**:

```rust
use std::cell::RefCell;
use std::rc::{Rc, Weak};

struct Node {
    name: String,
    parent: RefCell<Weak<Node>>,
    children: RefCell<Vec<Rc<Node>>>,
}

impl Node {
    fn new(name: &str) -> Rc<Node> {
        Rc::new(Node {
            name: name.to_string(),
            parent: RefCell::new(Weak::new()),
            children: RefCell::new(Vec::new()),
        })
    }
}

fn main() {
    let root = Node::new("root");
    let etc = Node::new("etc");
    *etc.parent.borrow_mut() = Rc::downgrade(&root);
    root.children.borrow_mut().push(Rc::clone(&etc));

    let parent_name = etc.parent.borrow().upgrade().map(|p| p.name.clone());
    println!("etc's parent: {parent_name:?}");
    println!("root: strong = {}, weak = {}", Rc::strong_count(&root), Rc::weak_count(&root));
}
```

```text
etc's parent: Some("root")
root: strong = 1, weak = 1
```

It's correct, and it carries a lot of ceremony: `Rc` for owning edges, `Weak` for back-edges (so the cycle doesn't leak,
as it did in Chapter 1.3), `RefCell` for mutation after construction, and `upgrade()` returning `Option` because the
parent might already be gone. Every line of that ceremony stands for a real question that Java's GC answers for you
implicitly.

The second option, usually better in systems code, is an **arena with indices**:

```rust
// A tree with parent links, the Rust way: nodes live in one Vec (the arena),
// links are indices. No Rc, no RefCell, no lifetime parameters.
struct Node {
    name: String,
    parent: Option<usize>,
    children: Vec<usize>,
}

#[derive(Default)]
struct Tree {
    nodes: Vec<Node>,
}

impl Tree {
    fn add(&mut self, name: &str, parent: Option<usize>) -> usize {
        let id = self.nodes.len();
        self.nodes.push(Node { name: name.to_string(), parent, children: Vec::new() });
        if let Some(p) = parent {
            self.nodes[p].children.push(id);
        }
        id
    }

    fn path(&self, mut id: usize) -> String {
        let mut parts = vec![self.nodes[id].name.as_str()];
        while let Some(p) = self.nodes[id].parent {
            parts.push(self.nodes[p].name.as_str());
            id = p;
        }
        parts.reverse();
        parts.join("/")
    }
}

fn main() {
    let mut tree = Tree::default();
    let root = tree.add("root", None);
    let etc = tree.add("etc", Some(root));
    let nginx = tree.add("nginx", Some(etc));
    println!("{}", tree.path(nginx));
    println!("root has {} child(ren)", tree.nodes[root].children.len());
}
```

```text
root/etc/nginx
root has 1 child(ren)
```

That's Fix 2 from Chapter 1.1 again: **store positions, not addresses.** It's contiguous in memory, has no refcounts,
and is trivially serializable. The costs move elsewhere: deleting nodes needs a free list, and stale indices need
generation counters (the `slotmap` crate packages exactly this). Many production Rust systems (compilers, ECS game
engines, graph databases) are built on this pattern. It's a real design shift for someone coming from Java, and it's
learnable.

**Cost 4: lifetime propagation.** A struct that borrows needs a lifetime parameter:

```rust,compile_fail
struct Request {
    method: String,
    path: &str,
}
```

```text
error[E0106]: missing lifetime specifier
 --> src/main.rs:4:11
  |
4 |     path: &str,
  |           ^ expected named lifetime parameter
  |
help: consider introducing a named lifetime parameter
  |
2 ~ struct Request<'a> {
3 |     method: String,
4 ~     path: &'a str,
```

The two ways out are two different architectures:

```rust
// Fix 1: own the data. Simple to hold anywhere; costs an allocation + copy per field.
struct Request {
    method: String,
    path: String,
}

// Fix 2: borrow from the input buffer. Zero-copy; but the lifetime 'a now
// appears in every type and function signature that holds a RequestRef.
struct RequestRef<'a> {
    method: &'a str,
    path: &'a str,
}

fn parse(raw: &str) -> Option<RequestRef<'_>> {
    let (method, path) = raw.split_once(' ')?;
    Some(RequestRef { method, path })
}

fn main() {
    let raw = String::from("GET /health");
    let borrowed = parse(&raw).expect("well-formed request line");
    let owned = Request {
        method: borrowed.method.to_string(),
        path: borrowed.path.to_string(),
    };
    println!("borrowed: {} {}", borrowed.method, borrowed.path);
    println!("owned:    {} {}", owned.method, owned.path);
}
```

Zero-copy parsing (Fix 2) is one of Rust's superpowers. Serde, HTTP parsers, and database drivers use it to avoid
allocating per field. But `'a` then appears in every struct that holds a `RequestRef`, and in every function that
returns one. Change the decision later and the edit ripples through the codebase. In Java this whole question doesn't
come up: every field is a reference, and the GC keeps the buffer alive. **Knowing when zero-copy is worth the lifetime
tax is an architect's call.** It's usually worth it on the hot path and rarely worth it everywhere else.

**Cost 5: async complexity** needs Part XII to explain properly. In brief: `async fn` returns a future that does nothing
until polled; you can't call async code from sync code without a runtime (**function coloring**); futures that borrow
across `.await` points bring `Pin` with them; and cancelling a task means **dropping its future mid-flight**, so any
`.await` is a point where your function may never resume. None of these is a flaw. Each is the cost of stackless,
runtime-agnostic, zero-allocation-by-default async. But they are costs, and Java's virtual threads avoid most of them by
choosing a different design.

---

## Pass 2 · Systems level — *Why are these costs structural, not accidental?*

### 4. Under the hood

**Why Rust compiles slowly.** It isn't carelessness. The design choices that make Rust fast at run time move work into
the compiler:

```text
 source ──► parse, expand macros ──► name resolution, type check, TRAIT SOLVING ──► BORROW CHECK (MIR)
                (proc macros are                                                           │
                 compiled AND run                                                          ▼
                 at build time)                                     MONOMORPHIZATION: one copy of each
                                                                    generic function per concrete type set
                                                                                          │
                                                                                          ▼
                                                  LLVM: optimize + codegen, per codegen unit  ──►  LINK
                                                  (inlining multiplies code before it shrinks)
```

- **Monomorphization** turns `Vec<T>` and every generic function into separate machine code for each `T` actually used.
  It's the source of zero-cost abstraction (Chapter 1.3) and also of large LLVM inputs.
- [RUSTC] **The crate is the compilation unit.** rustc splits a crate into *codegen units* so LLVM can work on them in
  parallel. But the front end (type checking, trait solving, borrow checking) is still largely sequential per crate on
  stable Rust. [VERSION] A parallel front end exists on nightly; check its status.
- **Procedural macros** (`#[derive(Serialize)]`, `#[tokio::main]`, `sqlx::query!`) are Rust programs that are compiled
  and then executed during your build.
- **Linking** a large statically linked binary takes real time. [VERSION] Since Rust 1.90, the faster `lld` linker is
  the default on `x86_64-unknown-linux-gnu`.

The mitigations are real, and Part VII and Part XX cover them: `cargo check` (type-check only, no codegen) for the
edit loop; incremental debug builds; splitting large crates so they build in parallel; keeping generic functions thin
(the "non-generic inner function" pattern); and measuring with `cargo build --timings`.

> **What actually happens?** Where does Java do all this work? **At run time, in production, on every instance.** `javac`
> is fast because it barely optimizes: it emits bytecode and leaves inlining, devirtualization, and register allocation
> to the JIT, which does them *while your service warms up*, on your production CPUs, every time a process starts. Rust
> does that work once, in CI. Neither is free. The question is whether you'd rather pay in build minutes or in warm-up
> latency and fleet CPU. For serverless functions and autoscaling, where processes start constantly, that question can
> decide the whole architecture.

### 5. Memory

**Ownership forms a tree. Everything else has to be modeled explicitly.**

```text
  JAVA: the heap is a GRAPH                     RUST: ownership is a TREE (a forest)
  (any object may point to any other;           (each value has exactly one owner; other
   the GC traces what's reachable)               relationships must be expressed explicitly)

        ┌──────┐                                        main
        │ root │◄──────────┐                              │ owns
        └──┬───┘           │ parent                     Tree
           │ children      │                              │ owns
        ┌──▼───┐           │                         Vec<Node>  ── [root][etc][nginx]
        │ etc  │───────────┘                                          ▲     │ parent: Some(0)
        └──┬───┘◄──────────┐                                          └─────┘  (an index,
           │               │ parent                                             not an owner)
        ┌──▼───┐           │
        │nginx │───────────┘                      non-owning edges:  &T  (borrow, scoped, checked)
        └──────┘                                                     Weak<T>  (may dangle → Option)
                                                                     index / ID  (checked on use)
```

This is the deepest adjustment for an engineer coming from the JVM. In Java you design objects and let the GC work out
lifetimes. In Rust you design **ownership first**: who owns what, which edges are temporary borrows, and which are IDs.
It feels like overhead at first. Over time it becomes a design tool, because it forces you to answer lifetime questions
that Java lets you postpone until the memory leak or the stale-cache bug shows up.

### 6. CPU / OS

**The scale lens: when does runtime efficiency actually matter?** Do the arithmetic before arguing about languages.

*Per-operation budget on one core (about 3 GHz):*

| Throughput per core | Time per operation | ≈ Cycles | What fits in the budget |
|---|---|---|---|
| 10K ops/s | 100 µs | ~300,000 | Almost anything: GC barriers, virtual calls, a few allocations, even a local syscall or two. **Language rarely matters.** |
| 1M ops/s | 1 µs | ~3,000 | A handful of cache misses, one or two allocations, no syscalls, no contended locks. **Design matters; the language starts to.** |
| 100M ops/s | 10 ns | ~30 | *One* DRAM miss is 5–10x the budget. No allocation, no locks, cache-resident data, often SIMD. **Layout is everything.** |

*Order-of-magnitude latencies (typical x86-64 servers; for reasoning, not quoting; measure your own hardware):*

| Operation | Rough cost |
|---|---|
| L1 cache hit | ~1 ns |
| L2 cache hit | ~4 ns |
| L3 cache hit | ~10–40 ns |
| DRAM access (cache miss) | ~80–120 ns |
| Uncontended mutex lock + unlock | ~10–25 ns |
| Atomic RMW on a cache line other cores are writing | ~50–100+ ns |
| Simple syscall | ~100 ns – 1 µs |
| Thread context switch | ~1–5 µs |
| NVMe random read | ~10–100 µs |
| Round trip within a datacenter | ~100–500 µs |
| Round trip across regions | ~30–150 ms |

Two consequences:

1. **A service that spends 20 ms waiting on the database won't be meaningfully faster in Rust.** Shaving 50 µs of CPU
   off a 20 ms request is a 0.25% improvement in latency. Language choice for such services should be driven by team,
   ecosystem, and correctness, not speed.
2. **"100M ops/s" is ambiguous.** Per core, it means hand-tuned in-memory loops (packet processing, matching-engine
   inner loops, stream aggregation), which is Rust/C/C++ territory. Across 64 cores it's about 1.5M ops/s per core,
   which well-written Java can often reach. **Always ask: per core, or per system?**

*Fleet arithmetic*, where efficiency turns into money: a gateway handling **400K req/s** at **0.5 ms of CPU per
request** keeps **200 cores** busy, or about **333 cores** at a 60% utilization target. If a rewrite cut CPU per request
by 30%, you'd save about 100 cores. Whether Rust gives you 30% for *your* workload is exactly what a prototype and a
benchmark must establish (Part XX). Don't assume it.

*Evidence other people have reported* (each is one organization's rewrite of one system, **not a controlled
experiment**, since rewrites also change architecture):

- **Cloudflare reported (2022)** that Pingora, its Rust HTTP proxy that replaced an NGINX-based service, used about
  70% less CPU and 67% less memory for the same traffic. Part of the gain came from architectural changes, such as
  sharing connections across threads, which a rewrite made possible.
- **Discord reported (2020)** that rewriting its Go "Read States" service in Rust eliminated latency spikes caused by
  Go's garbage collector. The service held a large cache of long-lived objects, and Go at the time forced a GC at least
  every two minutes, scanning all of it. That workload is close to a worst case for a tracing GC.

The pattern: the big reported wins come from **GC-hostile workloads** (large, long-lived heaps under tail-latency
requirements) and **per-request-CPU-bound fleets**. Neither describes the typical CRUD service.

**Binary size.** A Rust binary statically links its own std and monomorphized generics, so a "hello world" is hundreds
of KB (much of it debug info and symbols) rather than a few KB of C. It's still usually much smaller than a JVM plus its
jars. Profile settings shrink it: `strip`, `opt-level = "z"`, `lto`, `codegen-units = 1`, `panic = "abort"`
(Chapter 2.2).

---

## Pass 3 · Architect level — *When is Rust the right call, and when is it wrong?*

### 7. Trade-offs

**Rust usually earns its cost when at least one of these is true:**

1. **You need memory safety and no GC runtime at the same time.** Tail-latency SLOs on large heaps, hard memory
   ceilings (sidecars, edge, embedded), or no runtime allowed (kernels, firmware).
2. **Per-operation cost turns directly into money or SLO misses at scale.** Proxies, gateways, stream processors,
   storage engines, anything where the fleet-arithmetic column is large.
3. **You parse untrusted input in a component where a vulnerability would be severe.** Codecs, network protocol
   handlers, file-format parsers.
4. **The code must embed in other runtimes.** Libraries for Python (PyO3), Node, the JVM (via C ABI/FFM), WebAssembly,
   or C/C++ codebases.
5. **The code is long-lived infrastructure** where reliability and maintainability over years dominate initial velocity.

**Rust is probably the wrong choice when:**

1. **The bottleneck is I/O wait** (database, downstream services) and the team is productive in Java or Go.
2. **Time to market dominates** and the code is exploratory or short-lived.
3. **The domain is graph-heavy business logic** that changes constantly (rich domain models, workflow engines) and has
   no performance pressure.
4. **The ecosystem you depend on lives elsewhere.** Mature enterprise integration, specific SDKs, data science, and
   Kubernetes operator frameworks are all better served outside Rust today.
5. **Nobody on the team can mentor.** A Rust codebase written by a team still fighting the borrow checker tends to fill
   up with `clone()`, `Arc<Mutex<_>>`, and `unwrap()`, and ends up with Rust's costs and few of its benefits.

**Workload decision matrix** (a starting point for discussion, not a verdict):

| Workload | Java/Kotlin | Go | Rust |
|---|---|---|---|
| CRUD service, database-bound | **Strong** | **Strong** | Viable; rarely worth the cost |
| Complex, evolving domain logic | **Strong** | Good | Viable; slower to evolve |
| High-throughput proxy / gateway | Good (Netty) | **Strong** | **Strong** |
| Latency-critical, large in-memory state | Possible, with GC expertise | Risky (GC scanning) | **Strong** |
| CPU-bound data processing | Good (JIT) | Moderate | **Strong** |
| Parser for untrusted input | Good (safe; slower) | Good | **Strong** (safe *and* fast) |
| Library embedded in other languages | Poor | Poor (runtime) | **Strong** |
| CLI tools / DevOps tooling | Poor (startup) | **Strong** | **Strong** |
| Kernels, firmware, WASM | No | Limited | **Strong** |

> **Why not rewrite everything in Rust?** Because most of the benefit comes from **new** code. Google reported (2024)
> that vulnerability density drops sharply as code ages: most memory-safety bugs live in new or recently changed code.
> So switching *new development* to memory-safe languages cut Android's memory-safety vulnerabilities from 76% to 24%
> of the total over five years, without rewriting the existing C++. The same reasoning applies to reliability and
> performance: rewrite the component whose workload shape justifies it, and let stable old code stay stable.

> **Why not Java with ZGC?** Often, you should. Modern ZGC keeps pauses well under a millisecond on large heaps. The
> remaining costs are **memory headroom**, **concurrent GC CPU**, **allocation stalls** when the allocation rate
> outruns the collector, and warm-up. If those don't show up on your latency and cost dashboards, Java with a modern
> collector is a strong, low-risk choice.

### 8. Java comparison

**What you give up moving from Java to Rust:**

| Java capability | Rust situation |
|---|---|
| Reflection-driven frameworks (Spring DI, AOP, annotation scanning) | None at run time. Rust uses **compile-time code generation** (derive and attribute macros), which is explicit and fast, but less "magic." |
| Runtime introspection (JFR, heap dumps, attach agents, JMX) | `perf`, eBPF, `tracing`, tokio-console, core dumps: capable, but you can't inspect a live heap by type |
| Hot reload / class redefinition | No stable ABI [LANG]; plugins go through the C ABI, WASM, or separate processes |
| Exceptions with a stack trace by default | `Result` values carry no backtrace unless captured (`std::backtrace`, or `anyhow` with backtraces on) |
| Free-form object graphs | Ownership trees plus explicit non-owning edges (§5) |
| Very large hiring pool | Smaller and growing. Plan for training, not hiring alone. |

**What you gain:** deterministic resource management, much lower and more predictable memory, no warm-up, data-race
freedom in safe code, zero-copy designs the GC world makes awkward, and a compiler that checks refactors across the
whole codebase.

**The bridge.** You don't have to choose all-or-nothing. Java 22 finalized the **Foreign Function & Memory API** (FFM,
JEP 454), which calls native libraries without JNI's boilerplate. A Rust library with a C ABI can serve a Java service's
hot path while the rest stays in Java. Part XVI builds exactly that.

> **Analogy limit.** "Rust is like Java but faster" is wrong in both directions. Rust isn't a faster Java. It's a
> different **cost model**: explicit ownership instead of GC, compile-time generics instead of erasure, values instead
> of references by default. Code ported line by line from Java to Rust usually turns out slower *and* harder to read,
> because it copies the object-graph design into a language built around ownership trees. Good Rust starts from a
> different design, not a translated one.

### 9. Production scenario

**Meridian's adoption decision.** The platform team proposes "moving to Rust." The principal engineer asks for the
workload arithmetic first:

| Component | Where latency goes | Fleet CPU | Incident history | Decision |
|---|---|---|---|---|
| API gateway | ~0.5 ms CPU + TLS per request; p99 limited by GC pauses | ~330 cores | 2 GC-related SLO misses/quarter | **Rust prototype**; decide by benchmark |
| Ledger | 90% database time | 40 cores | Logic bugs, not memory | **Stay in Java** |
| Fraud features | 3 ms CPU-bound feature math inside a Java service | 120 cores | None | **Rust library via FFM**, only the hot loop |
| Market-data fan-out | Microsecond budgets | 60 cores | 1 memory-corruption incident (Chapter 1.1) | **Rewrite in Rust** |
| K8s operators | Negligible | ~0 | None | **Stay in Go** |

The decision isn't "move to Rust." It's **two targeted adoptions, one library, and two explicit non-adoptions**, each
justified by that component's workload. The success metrics are written down before any code: p99.9 at peak, cores per
1K req/s, memory per pod, and incident count. So are the exit criteria: if the gateway prototype doesn't reach a 25% CPU
reduction at equal p99 within eight weeks, stop.

### 10. Failure scenario

**The rewrite that didn't pay.** A composite drawn from common industry patterns (hypothetical, not a specific company):

```text
Month 0   "Our Java order service is slow. Let's rewrite it in Rust."   (No profile taken.)
Month 2   Team fights the borrow checker; converges on Arc<Mutex<T>> around every shared object,
          .clone() at every boundary, async everywhere "because it's faster".
Month 5   Feature parity at 80%. Java version kept shipping features; the gap widens.
Month 7   Benchmark: Rust version p50 equal, p99 slightly WORSE than Java.
          Profile finally taken: 85% of request time is PostgreSQL. Lock contention on one global Arc<Mutex<Cache>>.
Month 9   Project cancelled. Two engineers left. "Rust is overhyped" becomes team folklore.
```

Root causes, and not one of them is about Rust:

1. **No measurement before deciding.** The service was database-bound. Chapter 1.4's arithmetic would have said so on
   day one.
2. **A Java-shaped design written in Rust syntax.** Shared mutable object graphs behind locks threw away Rust's biggest
   lever, which is designing ownership so that sharing isn't needed.
3. **Big-bang scope.** No component boundary, no FFI bridge, no incremental path, and a moving target.
4. **No mentor.** Borrow-checker errors were treated as obstacles to get around (`clone()`, `Arc<Mutex>`) rather than as
   design feedback.

The lesson for an architect: **the way a Rust adoption fails is almost never "Rust couldn't do it."** It's choosing the
wrong component, measuring nothing, and designing the way you did in Java.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part I).*

1. Rust compiles slowly and Java compiles quickly. Where does Java do the equivalent optimization work, and what does
   that cost in production?
2. Why does a doubly linked list or a parent pointer feel hard in Rust? Describe three idiomatic designs and when you'd
   pick each.
3. A struct holds `&'a str` fields for zero-copy parsing. What's the benefit, what's the propagation cost, and how would
   you decide per module?
4. Walk through the per-operation budget arithmetic for a service at 1M requests/second across 32 cores. Is language
   choice likely to matter? What else would you need to know?
5. When is Java with ZGC the better choice than Rust for a latency-sensitive service? Give concrete conditions.
6. Why did Google's Android data suggest that writing *new* code in memory-safe languages beats rewriting old code?
   What does that imply for your adoption strategy?
7. A team says, "We rewrote it in Rust and it's slower." List the five most likely causes, in order of probability.
8. How would you introduce Rust into a 200-engineer Java organization without a big-bang rewrite? Name the technical
   bridge and the organizational steps.

### 12. Exercises

- **Beginner.** Rewrite the arena `Tree` so you can **remove** a node. What happens to indices held elsewhere? Sketch
  (in prose) how a generation counter fixes it.
- **Intermediate.** Take the `RequestRef<'a>` design and add a `Router` struct that stores the last 100 requests for
  debugging. What happens to the lifetime? Now do it with the owned `Request`. Which would you ship, and why?
- **Advanced.** Pick a real service you've worked on. Estimate (a) requests/second at peak, (b) CPU milliseconds per
  request, (c) the share of latency spent in I/O. Compute the per-core budget and the fleet core count, and write two
  sentences on whether a language change could plausibly matter.
- **Systems.** Measure JVM warm-up. Run a small Java HTTP handler under a constant load generator for 60 seconds and
  plot p99 latency per second. Where does it stabilize, and after how long? What would that curve cost in an autoscaling
  group that starts 50 instances during a traffic spike?
- **Architecture.** Write the "Rust is probably the wrong choice" list for *your* organization: five concrete
  conditions, each tied to a real system you know.

### 13. Debugging exercise

A junior engineer "fixed" a borrow-checker error by changing a struct field from `&str` to `String`. It compiles, and
it's slower in the hot path: 1.2M requests/second of header parsing, where every request now allocates about 12 small
strings.

```rust,compile_fail
struct Request {
    method: String,
    path: &str,
}
```

1. Explain the original E0106 error. What question was the compiler asking?
2. Compare the two fixes (owned `String` vs `RequestRef<'a>`) at 1.2M req/s: allocations per second, allocator
   contention across threads, and cache effects. Which would you choose for the parser's internal representation? For
   the representation handed to business logic?
3. Propose a **hybrid**: zero-copy inside the parsing layer and owned data at the boundary. Where exactly does the
   boundary go, and why there?

### 14. Design exercise

**A 12-month Rust adoption plan for Meridian.** You're the principal engineer. The organization has 12 backend teams
(all Java), one C++ team, and a mandate to "reduce memory-safety risk and infrastructure cost."

Deliver a two-page plan:

- **Which component first**, and why that one: workload arithmetic, blast radius, and boundaries.
- **The integration bridge**: a separate service over the network, an FFM library in-process, or a sidecar. Compare
  the three.
- **Skills**: who learns first, how code review works while nobody is yet fluent, and what "fluent" means in measurable
  terms.
- **Success metrics and exit criteria**, written down before any code.
- **What you'll explicitly *not* move to Rust**, and why.

There's no single right answer. A plan that honestly concludes "only the market-data service, and nothing else this
year" can beat an ambitious one if its reasoning is better.
