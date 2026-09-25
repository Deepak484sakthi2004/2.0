# Chapter 11.6 — Scoped Threads and Data Parallelism with Rayon

> **Where this sits:** Part XI · Concurrency · chapter 6 of 7
> **Prerequisites:** Chapter 11.1 (`spawn` needs `'static`), Chapter 11.2 (`Send`/`Sync`), Chapter 1.3's
> Leakpocalypse box, Project L1 (`logstat`), the Part IX interlude (BFS).
> **After this chapter you can:** let threads borrow local data safely with `thread::scope`, and explain why the
> pre-1.0 API that tried the same thing was unsound; parallelize CPU-bound work with rayon and predict when it won't
> help; split input so that per-thread results merge *exactly*; and parallelize a graph traversal level by level.

---

## Pass 1 · User level — *Borrowing across threads, and parallel loops*

### 1. Problem

Chapter 11.1 showed that `thread::spawn` needs `'static`: a detached thread may outlive the function that spawned it,
so it can't borrow that function's locals. For the most common kind of parallelism that requirement is pure friction.
You have a big `Vec` or a big input buffer, you want N threads to each work on part of it, and you want the combined
answer before the function returns. Wrapping the data in `Arc`, or cloning a chunk for each thread, costs memory and
obscures the code, and it's all to protect against a situation (the thread outliving the data) that the code's shape
already rules out.

The second problem is scheduling. Splitting work evenly by hand is easy for a `Vec<u32>` and hard for anything
irregular: lines of different lengths, graph frontiers, recursive divide-and-conquer. One slow chunk makes every other
thread wait. What you want is a pool that balances itself.

`std::thread::scope` solves the first problem. **Rayon** solves the second.

### 2. Mental model

```text
 thread::scope(|s| { ... })                          rayon
 ──────────────────────────                          ─────
 ┌─ scope ───────────────────────────────┐           one global pool of N workers (N = logical CPUs)
 │  s.spawn(|| uses &data)   ─┐          │           each worker has a DEQUE of tasks
 │  s.spawn(|| uses &mut part)├ threads  │             · owner pushes/pops at the bottom (LIFO: cache-warm)
 │  s.spawn(...)             ─┘ borrow   │             · idle workers STEAL from others' tops (FIFO: big pieces)
 │                                        │
 │  ── scope end: JOIN EVERY THREAD ──    │           join(a, b):  "a and b MAY run in parallel"
 └────────────────────────────────────────┘           par_iter():  recursive join over halves of the input
 borrows of data end here, after all threads          reduce/sum:  combine partial results (must be associative)
```

- **A scope is a region that no spawned thread can outlive** [LANG] [LIB]. `thread::scope` doesn't return until every
  thread spawned inside it has finished, so a scoped thread may borrow anything that outlives the scope, with ordinary
  `&` and `&mut` rules. It's Chapter 4's borrow checking, applied across threads.
- **Rayon turns "may run in parallel" into "runs in parallel if a core is free"** [LIB]. You describe *potential*
  parallelism (`join`, `par_iter`), and the pool decides, at run time, how much of it to use. Idle workers steal work,
  so uneven pieces balance themselves.
- **Parallel results equal sequential results when the combine step is associative**. Integer sums, histograms, counts,
  and set unions merge exactly. Floating-point sums don't (Chapter 7.2), and neither does anything order-dependent unless
  you keep the order.

### 3. Rust code

**Scoped threads borrow, first shared, then exclusive** (listing `ch06-01-scope-chunks.rs`):

```rust
use std::thread;

fn main() {
    let mut latencies_ms: Vec<u32> = (0..1_000_000u32).map(|i| i.wrapping_mul(7919) % 500).collect();
    let threads = thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let chunk = latencies_ms.len().div_ceil(threads);

    // Phase 1: shared borrows. Each thread reads its own chunk of the same Vec.
    let maxima: Vec<u32> = thread::scope(|s| {
        let handles: Vec<_> = latencies_ms
            .chunks(chunk)
            .map(|part| s.spawn(move || *part.iter().max().unwrap()))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let max = *maxima.iter().max().unwrap();

    // Phase 2: exclusive borrows. chunks_mut hands each thread a disjoint &mut [u32].
    thread::scope(|s| {
        for part in latencies_ms.chunks_mut(chunk) {
            s.spawn(move || {
                for x in part {
                    *x = *x * 1000 / max; // normalize to 0..=1000 in place
                }
            });
        }
    }); // all threads joined here: `latencies_ms` is ours again

    println!("{threads} threads; per-chunk maxima {maxima:?}; after normalizing, max = {}", latencies_ms.iter().max().unwrap());
}
```

```text
4 threads; per-chunk maxima [499, 499, 499, 499]; after normalizing, max = 1000
```

No `Arc`, no `clone`, no `'static`. Phase 2 is the interesting one: `chunks_mut` gives out **disjoint `&mut [u32]`**
slices, so four threads mutate one `Vec` in place, and the borrow checker verified that none of them overlap (Chapter
4.1's `split_at_mut`, one level up). After the scope, `latencies_ms` is exclusively ours again.

**Rayon: the same answers, in parallel** (listing `ch06-03-rayon-basics.rs`, excerpt):

```rust,ignore
/// Divide and conquer with rayon::join: "these two may run in parallel if a thread is free".
fn sum(xs: &[u64]) -> u64 {
    if xs.len() <= 4_096 {
        return xs.iter().sum();
    }
    let (left, right) = xs.split_at(xs.len() / 2);
    let (a, b) = rayon::join(|| sum(left), || sum(right));
    a + b
}
// ...
let par: u64 = amounts.par_iter().map(|x| x % 97).sum();
prices.par_chunks_mut(10_000).for_each(|chunk| chunk.sort_unstable()); // sort each chunk in parallel
prices.par_sort_unstable(); // parallel sort of the whole thing
let first_big = amounts.par_iter().find_first(|&&x| x > 999_990);
```

```text
rayon global pool: 4 threads
par_iter sum == sequential: true (47999082)
rayon::join sum: 500000500000
chunks sorted: true; whole vector sorted: true
filter/count: 1000; find_first: Some(999991)
```

Changing `iter()` to `par_iter()` is the whole API for the common case. `find_first` keeps sequential semantics (the
*first* match in order, not whichever thread wins) at the cost of some coordination. `find_any` is the faster,
nondeterministic version.

**Parallel `logstat`** (the promise from Project L1's review, question 7). Split the input at newline boundaries,
build one `Summary` per chunk with **no shared state at all**, then merge (listing `ch06-04-parallel-logstat.rs`,
excerpts):

```rust,ignore
/// Up to `parts` chunks, each ending just after a b'\n' (or at the end). A newline byte never occurs inside
/// a multi-byte UTF-8 sequence, so no line, and no character, is ever cut in two.
fn split_lines(input: &[u8], parts: usize) -> Vec<&[u8]> { /* ... */ }

fn parallel_scoped(input: &[u8], threads: usize) -> Summary {
    thread::scope(|s| {
        let handles: Vec<_> = split_lines(input, threads).into_iter().map(|c| s.spawn(move || ingest(c))).collect();
        handles.into_iter().map(|h| h.join().unwrap()).fold(Summary::new(), Summary::merge)
    })
}

fn parallel_rayon(input: &[u8]) -> Summary {
    // More chunks than threads: work stealing evens out chunks that happen to be slower.
    split_lines(input, rayon::current_num_threads() * 8).into_par_iter().map(ingest).reduce(Summary::new, Summary::merge)
}
```

`Summary::merge` adds every field: counters, the latency histogram element by element, and the per-path map key by
key. Every part of `Summary` is a sum, so the merge is **exact**. The listing asserts that all three strategies produce
identical summaries, and two tests check the splitter and the merge:

```text
identical summaries from 3 strategies: true
lines: 1000000 (800 malformed, 200 invalid UTF-8)
status: 1xx=0 2xx=499475 3xx=99637 4xx=199718 5xx=200170
p50=Some(203) p99=Some(3015) ms; top 3 paths [("/api/items/18", 20346), ("/api/items/20", 20227), ("/api/items/25", 20168)]
input 46 MB; sequential 81.9ms, scoped x4 36.2ms, rayon 27.9ms (one run)
```

That's why Project L1 kept a **bounded histogram** instead of a sorted list of latencies. A histogram merges by
addition, so percentiles over the merged histogram are exactly the sequential percentiles. A per-chunk p99 can't be
combined into a global p99 at all. The data structure chosen in Part II is what makes the parallel version correct.

---

## Pass 2 · Systems level — *Why scopes are sound, and how stealing works*

### 4. Under the hood

**The signature that makes it sound** [LIB]:

```rust,ignore
pub fn scope<'env, F, T>(f: F) -> T
where
    F: for<'scope> FnOnce(&'scope Scope<'scope, 'env>) -> T;

impl<'scope, 'env> Scope<'scope, 'env> {
    pub fn spawn<F, T>(&'scope self, f: F) -> ScopedJoinHandle<'scope, T>
    where
        F: FnOnce() -> T + Send + 'scope,
        T: Send + 'scope;
}
```

Spawned closures need only `'scope`, not `'static`, so they may borrow anything that outlives the scope (`'env`). The
higher-ranked `for<'scope>` (Chapter 4.5) stops the `Scope` handle itself from escaping the closure. The crucial design
choice is that **`scope` joins the threads itself, inside the function, before it returns**, including when `f`
panics: it waits for every thread, and then resumes the panic. Soundness depends on **control flow**, which safe code
can't skip, and not on a destructor, which safe code *can* skip.

That distinction is the Leakpocalypse from Chapter 1.3's box. Before 1.0, std had `thread::scoped`, which returned a
`JoinGuard` whose `Drop` joined the thread. In 2015 it was pointed out that `mem::forget` is safe (and so are
`Rc` cycles), so the destructor might never run, and the thread would keep reading a dead stack frame. Here is the old
design rebuilt in miniature, with the lie in its `SAFETY` comment spelled out (listing `ch06-02-leakpocalypse.rs`,
excerpt):

```rust,ignore
pub fn scoped<'a, F: FnOnce() + Send + 'a>(f: F) -> JoinGuard<'a> {
    let job: Box<dyn FnOnce() + Send + 'a> = Box::new(f);
    // SAFETY (claimed): the closure can't outlive 'a, because JoinGuard<'a> joins in Drop.
    // That claim is FALSE: destructors are not guaranteed to run (mem::forget, Rc cycles).
    let job: Box<dyn FnOnce() + Send + 'static> = unsafe { std::mem::transmute(job) };
    JoinGuard { handle: Some(thread::spawn(job)), _borrows: PhantomData }
}

fn start_report() {
    let totals = vec![120u64, 45, 300];
    let guard = scoped(|| {
        thread::sleep(Duration::from_millis(50));
        let sum: u64 = totals.iter().sum(); // `totals` belongs to a stack frame that is gone
        // ...
    });
    std::mem::forget(guard); // safe code: no join, no error
} // `totals` is dropped here, while the thread still borrows it
```

Miri reports the use-after-free on the spawned thread (`debug miri` check):

```text
error: Undefined Behavior: constructing invalid value of type {closure@src/main.rs:36:24: 36:26}: at .<captured-var(totals)>, encountered a dangling reference (use-after-free)
  --> src/main.rs:36:24
   |
36 |       let guard = scoped(|| {
   |  ________________________^
...
   = note: this is on thread `unnamed-1`
```

The fix at the time was to remove the API (it never reached stable), make `mem::forget` officially safe (RFC 1066), and
adopt the rule that **destructors may be used for cleanup, never for soundness**. Crates such as crossbeam then offered
closure-based scopes, and std stabilized `thread::scope` in Rust 1.63 (2022) [VERSION]. Part XV's safety-invariant
discipline starts from this story.

**Rayon's scheduler** [LIB] (the design described in its documentation and in its author's 2015 introduction; details
change between versions):

```text
 worker k's deque:   [ oldest ... newest ]
                       ▲ thieves steal here      ▲ owner pushes/pops here

 join(a, b) on worker k:
   1. push b onto k's deque              (b is now visible to thieves)
   2. run a right here                   (no hand-off: a is warm in k's cache)
   3. pop the bottom of the deque:
        still b?  → run b here too       (nobody was idle: zero parallelism overhead beyond the push/pop)
        stolen?   → while b runs elsewhere, steal and run other work instead of blocking
   4. return (result_a, result_b)
```

`par_iter()` is recursive `join` over halves of the input, with **adaptive splitting**: it splits into roughly as many
pieces as there are threads, and splits further only when pieces get stolen, which is a sign of idle workers. Thieves
take the *oldest* entry, which in a divide-and-conquer tree is the *biggest* remaining piece, so one steal moves a lot of
work. The deques are crossbeam's Chase–Lev work-stealing deques.

Two consequences matter for design:

- **Work stealing balances irregular work automatically.** The `rayon` row beat the scoped row in the logstat run
  (27.9 vs 36.2 ms) because it had 32 chunks for 4 threads and stealing evened them out. With exactly 4 chunks, the
  slowest one decides the total.
- **The pool's threads are shared by everything in the process** that uses the global pool. Blocking one of them
  (I/O, a lock held by someone who's waiting for rayon, a `recv`) takes a core away from every other parallel job
  (§10).

### 5. Memory

| What | Cost | Note |
|---|---|---|
| A scoped thread | A full OS thread: 2 MiB stack mapping + kernel task (11.1) | Created and joined per scope: ~28 µs each (measured in 11.1) |
| Rayon's pool | N threads, created once, on first use | Reused by every `par_iter` in the process |
| A rayon task (`join`) | A small job on the deque; closures usually live on the caller's stack | No allocation per `join` in the common case [LIB] |
| Per-thread `Summary` in logstat | One histogram (10,002 × 8 B = 80 KB) + a path map, per chunk | Memory grows with the number of *chunks*, not the input size |
| Parallel BFS | `Vec<AtomicU32>` = 4 B per node (same layout as `u32`), plus frontier `Vec`s | 2M nodes → 8 MB of distances |

The per-thread-state pattern is the memory trade that buys speed: **N private copies of a small structure** instead of
one shared structure behind a lock. It pays whenever the structure is small compared with the input. It would not pay
for a per-thread copy of a 10 GB index. There, the right design is shared read-only data (`&` borrows in a scope) plus
private outputs.

### 6. CPU / OS

**Speedups measured on the Playground's 4 cores** (release; one run each; the shared machine makes these noisy):

| Workload | Sequential | Parallel | Speedup | Why not 4× |
|---|---|---|---|---|
| logstat, 1M lines, 46 MB (`ch06-04`) | 81.9 ms | scoped ×4: 36.2 ms; rayon: 27.9 ms | 2.3× / 2.9× | parsing is memory-bound; merge is sequential; uneven chunks (scoped) |
| BFS, 2M nodes, 18M edges (`ch06-05`) | 363.9 ms | 196.5 ms | 1.9× | random memory access (Chapter 9.5); CAS traffic on `dist`; per-level barriers |

Earlier runs of the same listings gave 82–84 ms / 30–40 ms / 28–42 ms for logstat and 354.8 / 272.2 ms for BFS. Speedup
is a range, not a number, and it's always below the core count. The usual suspects: **memory bandwidth** (4 cores
share one memory system [CPU]), **the sequential parts** (Amdahl's law: the merge, the splitting, the level barriers),
**load imbalance**, and **coherence traffic** on shared atomics.

**Parallelism has a fixed cost per call.** Waking workers, splitting, stealing, and joining take microseconds, so small
inputs get *slower* (listing `ch06-06-par-overhead.rs`, a `sqrt` per element, release, one run):

```text
         n     sequential       par_iter   speedup
       100         0.2 µs         8.1 µs     0.03x
      1000         2.4 µs        13.0 µs     0.18x
     10000        21.9 µs        28.7 µs     0.76x
    100000       219.8 µs       138.9 µs     1.58x
  10000000     22847.3 µs     10608.7 µs     2.15x
```

(An earlier run: 0.04×, 0.11×, 0.64×, 1.43×, 2.77×.) For this cheap per-element work, the break-even is somewhere
between 10,000 and 100,000 elements, around **tens of microseconds of total work**. The rule of thumb: **parallelize
work measured in milliseconds, not in elements.** A `par_iter` over 100 items in a request handler makes that request
slower and every other request's parallel work noisier.

**The parallel BFS** (the Part IX interlude's promise) is level-synchronous. It expands the whole frontier in
parallel, and each newly reached node is claimed by exactly one `compare_exchange` (listing `ch06-05-parallel-bfs.rs`,
excerpt):

```rust,ignore
frontier = frontier
    .par_iter()
    .flat_map_iter(|&u| {
        g.neighbors(u).iter().copied().filter(|&v| {
            dist[v as usize].compare_exchange(UNSEEN, level + 1, Ordering::Relaxed, Ordering::Relaxed).is_ok()
        })
    })
    .collect();
```

```text
graph: 2000000 nodes, 18000000 edges; rayon threads: 4
same distances: true
frontier size per level: [1, 9, 81, 729, 6547, 57870, 444096, 1296191, 194338, 138]
sequential 363.9ms, parallel 196.5ms (one run)
```

The frontier sizes explain the speedup's shape. Levels 0–4 have fewer than 7,000 nodes, too small to parallelize well
(the table above), and almost all the work is in levels 6–7, with 444K and 1.3M nodes. The CAS makes the *distances*
deterministic even though the order in which threads discover nodes isn't: whichever thread wins, the value it writes
is `level + 1`. `Relaxed` is enough because the `collect` at the end of each level waits for every task, and that join
orders the levels. Part XIV explains why that's sufficient.

---

## Pass 3 · Architect level — *Which parallelism, and where it goes wrong*

### 7. Trade-offs

| Criterion | `thread::scope` | rayon | `spawn` + `Arc` / channels |
|---|---|---|---|
| **Memory** | N OS threads per scope | One pool per process | Threads you manage |
| **CPU** | ~28 µs per thread per scope | µs per parallel call; per-`join` cost tiny | Hand-off ~5 µs per message |
| **Latency** | Scope waits for the slowest thread | Work stealing shortens the tail | Depends on the pipeline |
| **Throughput** | Good for few, even, long tasks | Best for many or irregular CPU tasks | Best for streaming stages |
| **Contention** | Whatever the threads share | Whatever the closures share | Queue heads/tails |
| **Cache behavior** | Each thread keeps its chunk warm | LIFO own-deque keeps work warm | Data moves per message |
| **Allocation** | Thread stacks per scope | Rare | Per message (boxes, blocks) |
| **Complexity** | Low: plain borrows | Lowest: `par_iter` | Higher: protocols, shutdown |
| **Safety** | Borrow-checked | Borrow-checked, plus `Fn + Send + Sync` closures | `'static` + ownership transfer |
| **Maintainability** | Explicit threads | Hides the threads (good and bad) | Explicit topology |
| **Failure modes** | A panic in one thread → scope panics | Blocking inside the pool starves it (§10) | Unbounded queues, stuck shutdown |
| **Operational implications** | Thread count = what you wrote | `RAYON_NUM_THREADS`, one global pool | Queue depths as metrics |

The quick decision: **CPU-bound work on data you already have → rayon. A handful of long-running helpers that need to
borrow → `thread::scope`. Work that arrives over time, or I/O → a pool with a queue (Project L3), channels, or async
(Part XII).**

### 8. Java comparison

| Java | Rust | Note |
|---|---|---|
| `list.parallelStream()` | `vec.par_iter()` | Both run on a shared pool; rayon's closures must be `Fn + Send + Sync` |
| `ForkJoinPool.commonPool()` (parallelism = CPUs − 1, per the JDK docs) | rayon's global pool (threads = CPUs) | Java's caller thread also participates |
| `RecursiveTask.fork()/join()` | `rayon::join` | Same work-stealing idea (Doug Lea's fork/join, 2000) |
| `Spliterator` | rayon's `Producer`/`split_at` | How the input is divided |
| Custom `ForkJoinPool` for isolation | `rayon::ThreadPoolBuilder` + `pool.install(..)` | Isolation for blocking or priority work |
| `StructuredTaskScope` (preview since JDK 21, JEP 453 and successors) [VERSION] | `thread::scope` | Rust's is stable and checks borrows at compile time |
| `invokeAll(tasks)` + `Future.get()` | scope + `ScopedJoinHandle::join` | Results travel by ownership |

The classic parallel-stream bug is mutating captured state from inside the stream:
`list.parallelStream().forEach(x -> total[0] += x)`, the one-element-array trick for getting around "effectively
final". It compiles, it runs, and it loses updates. The rayon version doesn't compile (listing
`ch06-07-par-mutation.rs`):

```rust,compile_fail
use rayon::prelude::*;

fn main() {
    let amounts: Vec<u64> = (1..=1_000).collect();
    let mut total = 0u64;
    amounts.par_iter().for_each(|x| total += x);
    println!("{total}");
}
```

```text
error[E0596]: cannot borrow `total` as mutable, as it is a captured variable in a `Fn` closure
 --> src/main.rs:9:37
  |
8 |     let mut total = 0u64;
  |         --------- `total` declared here, outside the closure
9 |     amounts.par_iter().for_each(|x| total += x);
  |                        -------------^^^^^------
  |                        |        |   |
  |                        |        |   cannot borrow as mutable
  |                        |        in this closure
  |                        expects `Fn` instead of `FnMut`
```

`for_each` requires `Fn`, because the same closure is called from several threads at once, and an `Fn` closure can't
mutate its captures. The fixes are the parallel idioms: `.sum()`, `.reduce()`, or per-thread state merged at the end
(logstat), or an `AtomicU64` if you really need shared counting (11.7 shows what that costs).

> **Analogy limit.** "rayon is parallel streams" is right about the programming model and the scheduler, and wrong
> about what the compiler checks. Java's `forEach(Consumer)` accepts any lambda. Whether it's safe to run concurrently
> is documented ("non-interfering, stateless") but not checked. Rayon puts the same rules in the type: `Fn` (no
> mutation of captures), `Send` (may move to a worker), `Sync` (may be shared by several workers). A rayon closure that
> compiles can't have the lost-update bug.

### 9. Production scenario

**Meridian's nightly settlement reconciliation.** The settlement batch job (the Rust service from Chapter 8.1) matches
the day's ledger entries against the card processors' settlement files: about 40 million records on a peak day, in a
window that has to finish before partner cutoffs. The first version was single-threaded and took about 70 minutes.
That was fine until Black Friday volumes pushed it past the window.

The parallel version is logstat's design at scale:

- **Partition by merchant, not by line**, so each partition's matching is independent. The input files are sorted by
  merchant, so splitting at merchant boundaries works exactly like `split_lines`'s newline rule, and nothing that must
  be matched together is ever in two partitions.
- **Per-partition results with no shared state**: matched counts, a mismatch list, and per-currency totals as exact
  integer minor units (Chapter 2.3). The merge is addition and concatenation, so the result is **identical to the
  sequential run**, which is how they tested it: sequential and parallel runs are compared byte for byte on a week of
  production files.
- **rayon with 8× more partitions than cores**, because merchant sizes are extremely uneven (the largest merchant is
  about 4% of volume) and work stealing absorbs the skew.
- **Determinism rules**: mismatches are sorted before output, sums never use floats, and the report doesn't depend on
  which thread finished first.

The run dropped to about 14 minutes on 8 cores. That's not 8×, because the file reads and the final report are
sequential, but it's inside the window with room to spare.

### 10. Failure scenario

**Rayon in the request path.** A Meridian merchant-portal service rendered statement pages that combined 30–50 documents
fetched from an internal document store. A developer made the fetch loop parallel with `ids.par_iter().map(fetch)`,
where `fetch` was a blocking HTTP call of 20–200 ms. It looked great in a load test with one user: page time dropped
from 2 s to 300 ms.

In production, with a few hundred concurrent users, it went wrong in three ways:

1. **Rayon's global pool has as many threads as cores (8 here), and blocking calls occupy them.** Eight in-flight
   fetches filled the whole pool. Every other `par_iter` in the process, including the PDF renderer's genuinely
   CPU-bound work, queued behind network calls.
2. **Nested parallelism amplified it.** The renderer called `par_iter` from inside work that was itself running on the
   pool. Those inner tasks were stuck behind outer tasks that were waiting on the network.
3. **Page latency became erratic.** p50 improved, but p99 went from 2.5 s to over 9 s, because a request's fetches now
   waited for *other* requests' fetches to release pool threads.

The fix was to separate the kinds of work:

- **I/O concurrency is not CPU parallelism.** The fetches moved to async (Part XIII) with a concurrency limit per request.
  A dedicated blocking pool with a bounded queue (Project L3's design) would also have worked.
- **Rayon stays for CPU-bound work only**, and CPU-heavy requests get their own `ThreadPoolBuilder` pool, so a
  rendering spike can't starve the rest of the process.
- **A review rule**: no blocking call (I/O, `recv`, `lock` on a contended mutex, `thread::sleep`) inside `par_iter`
  or `rayon::join`. It's the same rule async executors have (Chapter 13.2), for the same reason.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XI).*

1. Why can a thread spawned in `thread::scope` borrow a local `Vec`, when one spawned by `thread::spawn` can't?
2. What was unsound about the pre-1.0 `thread::scoped` API? Why is `thread::scope` sound where it wasn't?
3. How does `thread::scope` behave if one of its threads panics? If `f` itself panics?
4. Explain rayon's `join` in terms of deques. Why does the owner pop from one end and thieves steal from the other?
5. Why did the rayon version of logstat beat the scoped version with the same 4 threads?
6. Why is logstat's parallel output *exactly* equal to the sequential output? Which statistic would break that property?
7. At what input size did `par_iter` start to pay off in this chapter, and why is there a threshold at all?
8. Why is it wrong to make blocking I/O calls inside `par_iter`?
9. The parallel BFS uses `compare_exchange` with `Relaxed`. What makes that correct, and what makes the result
   deterministic?
10. Compare Java parallel streams with rayon: the pool, the scheduling, and what each checks at compile time.

### 12. Exercises

- **Beginner.** Rewrite a sequential "count words per line" loop over a `Vec<String>` three ways: `thread::scope` with
  four chunks, `par_iter().map().sum()`, and `par_iter().fold().reduce()`. Check that all three agree.
- **Intermediate.** Add a `distinct_paths` statistic to logstat's `Summary` that is still exactly mergeable. Then try
  `p99_per_chunk` and show that averaging per-chunk p99s gives the wrong global p99 on a skewed input.
- **Advanced.** Implement a parallel merge sort with `rayon::join` and a sequential cutoff. Measure the cutoff that
  minimizes time for 10M `u64`s, and compare it with `par_sort_unstable`.
- **Systems.** Run `ch06-04-parallel-logstat.rs` locally with `RAYON_NUM_THREADS=1,2,4,8` and plot the speedup. Where
  does it flatten? Use `perf stat -e cache-misses` to test the "memory-bound" explanation.
- **Architecture.** Meridian wants the reconciliation job to also produce a p99 settlement delay per merchant. Design
  the per-partition data structure so the result stays exactly mergeable and bounded in memory.

### 13. Debugging exercise

A service's CPU-heavy endpoint sometimes hangs forever under load. A thread dump shows every rayon worker blocked
trying to lock `CACHE`:

```rust,ignore
static CACHE: LazyLock<Mutex<HashMap<u64, Arc<Model>>>> = LazyLock::new(Default::default);

fn score_all(users: &[User]) -> Vec<f64> {
    users.par_iter().map(|u| {
        let model = {
            let mut cache = CACHE.lock().unwrap();                   // (1)
            Arc::clone(cache.entry(u.segment).or_insert_with(|| {
                Arc::new(train_model(u.segment))                    // (2) uses par_iter internally
            }))
        };
        model.score(u)
    }).collect()
}
```

1. `train_model`'s inner `par_iter` runs while the guard from (1) is alive. While a rayon worker waits for its inner
   tasks, it doesn't block: it runs *other* tasks (§4). Explain how the worker holding `CACHE` can end up trying to
   lock `CACHE` again, and why the other workers then pile up behind it.
2. Even on a day when it doesn't hang, what does holding `CACHE`'s lock around `train_model` do to the endpoint's
   parallelism?
3. Restructure it: find the missing segments first, train them in parallel outside any lock, publish them, then score.
   Which of Chapter 11.3's review rules would have caught this?

### 14. Design exercise

**Fraud-model backfill.** When a new fraud model ships, Meridian recomputes features for the last 90 days of
transactions: about 1.2 billion rows, stored as daily files of 10–20 GB each, on a 32-core machine with 128 GB of RAM.
Features for a card depend on that card's previous transactions *in time order*.

- How do you partition the work so that each partition is independent? (What does "time order per card" rule out?)
- Where do you use rayon, where do you use a pipeline with channels (reading files, parsing, computing, writing), and
  why?
- What per-thread state do you keep, and how does it merge? What's your memory budget per thread?
- How do you make the output deterministic, so a rerun produces byte-identical files?
