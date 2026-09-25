# Chapter 10.4 — Rust Iterators vs Java Streams

> **Where this sits:** Part X · Closures, Iterators, and Zero-Cost Abstractions · chapter 4 of 4
> **Prerequisites:** Chapters 10.1–10.3; Chapter 7.2 (erasure, boxing, and JIT profiles); Chapter 8.1 (`Result`,
> `?`).
> **After this chapter you can:** translate Java stream code into idiomatic Rust, including the `Collectors` that have no
> one-line equivalent; handle errors inside pipelines without the checked-exception contortions of Java lambdas; say
> where the two models' semantics differ (duplicate keys, reuse, ordering, floating point) before a port bites you; and
> choose between sequential iterators and Rayon's parallel ones.

---

## Pass 1 · User level — *Same shape, different contracts*

### 1. Problem

A Java engineer porting code to Rust meets iterator chains on day one and feels at home: `filter`, `map`, `collect`,
lazy evaluation, a terminal operation. The similarity is real, and it's also the risk. The operations *look* the same
and mostly *behave* the same, so the places where they differ get ported without a second look. `Collectors.toMap`
throws on a duplicate key, and `collect::<HashMap<_, _>>()` doesn't. A stream can't be reused, but a cloned iterator
can. A lambda can't throw a checked exception, but a closure can return a `Result`, which changes how the whole
pipeline is shaped. A parallel stream runs on a pool you share with the rest of the JVM.

This chapter is the translation guide: the mapping, the mechanisms underneath each side, and the semantic differences
that have caused real incidents.

### 2. Mental model

```text
 JAVA STREAM                                          RUST ITERATOR
 ──────────────────────────────────────────────       ─────────────────────────────────────────────────
 a linked list of stage OBJECTS on the heap            one nested VALUE on the stack (Chapter 10.3)
 built lazily; nothing runs until a terminal op        built lazily; nothing runs until a consumer
 terminal op PUSHES elements through a chain of        consumer PULLS with next(), or lets the chain
   Sink objects (short-circuit ops pull via             push through fold() (internal iteration):
   tryAdvance)                                          both models exist in one trait
 ONE-SHOT: reuse throws IllegalStateException          consumed by value: reuse is a compile error;
                                                        clone() a borrowing iterator to traverse twice
 elements are objects (Stream<Long>) unless you        elements are whatever T is; no boxing unless
   switch to IntStream/LongStream/DoubleStream           you write Box
 errors: unchecked exceptions escape the pipeline;     errors: values in the item type (Result<T, E>);
   checked ones must be wrapped                          collect/try_fold/? decide what happens
 .parallel(): same pipeline, common ForkJoinPool       .par_iter() (Rayon): a different trait,
                                                        global work-stealing pool
```

The deepest difference is the last-but-one row. In Java, failure travels **beside** the data, as an exception that
unwinds out of the terminal operation. In Rust, failure travels **with** the data, as `Result` items, and the consumer
you choose decides the policy: stop at the first error, skip errors, or collect them.

### 3. Rust code

**Collecting into `Result`: all or the first error** (listing `ch04-01-collect-result.rs`):

```rust
// Collecting into Result/Option short-circuits at the first failure: the rest is never parsed.
use std::cell::Cell;

fn main() {
    let parsed = Cell::new(0);
    let parse = |s: &str| {
        parsed.set(parsed.get() + 1);
        s.parse::<u64>().map_err(|e| format!("bad amount {s:?}: {e}"))
    };

    let good = ["1200", "550", "99"];
    let bad = ["1200", "5x0", "99", "7", "8"];

    let ok: Result<Vec<u64>, String> = good.iter().map(|s| parse(s)).collect();
    println!("{ok:?}  (parsed {})", parsed.replace(0));

    let err: Result<Vec<u64>, String> = bad.iter().map(|s| parse(s)).collect();
    println!("{err:?}  (parsed {} of {})", parsed.replace(0), bad.len());

    // sum / product also accept Result and Option items:
    let total: Result<u64, String> = good.iter().map(|s| parse(s)).sum();
    println!("sum = {total:?}");

    // Option: all present, or None at the first gap.
    let limits = [Some(500_u32), None, Some(100)];
    let all: Option<Vec<u32>> = limits.iter().copied().collect();
    println!("all limits present? {all:?}");
}
```

```text
Ok([1200, 550, 99])  (parsed 3)
Err("bad amount \"5x0\": invalid digit found in string")  (parsed 2 of 5)
sum = Ok(1849)
all limits present? None
```

`collect::<Result<Vec<_>, _>>()` stopped after the second element, and the counter proves the rest was never parsed.
That's the Rust spelling of "a checked exception aborts the loop," without the exception. The `Cell` counter is Chapter
10.1's interior-mutability pattern: the `parse` closure is `Fn`, and it can still count.

**`try_fold`: a fold that can stop** (listing `ch04-02-try-fold.rs`, abridged):

```rust,ignore
    // Release payouts in order until the next one would exceed the budget.
    let mut released = Vec::new();
    let outcome = queue.iter().try_fold(0_u64, |spent, p| {
        let next = spent + p.cents;
        if next > daily_budget {
            ControlFlow::Break((spent, p.merchant)) // stop: report where we stopped
        } else {
            released.push(p.merchant);
            ControlFlow::Continue(next)
        }
    });
```

```text
released ["m-17", "m-03"]
outcome  Break((75000, "m-44"))
checked total: Err("overflow adding 4 to 18446744073709551613")
```

`try_fold` is the most general consumer in the trait. Its closure returns anything that implements the `Try`
protocol (`ControlFlow`, `Result`, `Option`), and the first `Break`/`Err`/`None` ends the loop. [LIB] std builds
`any`, `all`, `find`, `position`, and friends on top of it, and adapters override it just like `fold` (Chapter 10.3),
so an early-exit pipeline still gets each adapter's best loop shape. The second output line uses `Result`:
`checked_add` turns overflow into an error instead of the silent wrap of Chapter 2.2's discount incident.

**Java's `Collectors`, translated** (listing `ch04-03-collectors.rs`, abridged):

```rust,ignore
    // groupingBy(merchant, summingLong(cents)): a fold into a map (BTreeMap for sorted output).
    let by_merchant: BTreeMap<&str, u64> = txns.iter().fold(BTreeMap::new(), |mut m, t| {
        *m.entry(t.merchant).or_insert(0) += t.cents;
        m
    });

    // partitioningBy(refunded)
    let (refunded, kept): (Vec<Txn>, Vec<Txn>) = txns.iter().partition(|t| t.refunded);

    // joining(", ") over distinct merchants, in first-seen order
    let mut seen = HashSet::new();
    let merchants: Vec<&str> = txns.iter().map(|t| t.merchant).filter(|m| seen.insert(*m)).collect();

    // unzip: one pass, two collections
    let (names, amounts): (Vec<&str>, Vec<u64>) = txns.iter().map(|t| (t.merchant, t.cents)).unzip();

    // a custom collector
    let hist: Histogram = txns.iter().map(|t| t.cents).collect();
```

```text
sum by merchant: {"acme": 9350, "beta": 250000, "zeta": 99000}
refunded=1 kept=4
merchants: acme, zeta, beta
names=["acme", "zeta", "acme", "beta", "acme"] amounts=[1250, 99000, 400, 250000, 7700]
Histogram { buckets: [1, 2, 1, 1] }
```

There's no `groupingBy`. Its Rust form is a `fold` into a map with the entry API (Chapter 9.3: one hash per element).
It's more code, and the accumulator is explicit. A **custom collector** is any type implementing `FromIterator`. The
listing's `Histogram` implements `FromIterator<u64>` in 15 lines, and `collect()` targets it by type. Java's
`Collector.of(supplier, accumulator, combiner, finisher)` needs four functions, because it must support parallel
combination. Rust splits that concern out: sequential `collect` needs only `FromIterator`, and Rayon's parallel
collect has its own traits.

**Reuse is a compile-time error, and cloning is the fix** (listings `ch04-05-iterator-reuse.rs` and
`ch04-06-clone-iterator.rs`):

```rust,compile_fail
fn main() {
    let amounts = vec![1_200_u64, 550, 99];
    let big = amounts.iter().filter(|&&a| a > 100);
    let count = big.count(); // count() takes the iterator by value: consumed
    let total: u64 = big.sum(); // Java: IllegalStateException at run time; Rust: rejected at compile time
    println!("{count} {total}");
}
```

```text
error[E0382]: use of moved value: `big`
 --> src/main.rs:6:22
  |
4 |     let big = amounts.iter().filter(|&&a| a > 100);
  |         --- move occurs because `big` has type `Filter<std::slice::Iter<'_, u64>, {closure@src/main.rs:4:37: 4:42}>`, which does not implement the `Copy` trait
5 |     let count = big.count(); // count() takes the iterator by value: consumed
  |                     ------- `big` moved due to this method call
6 |     let total: u64 = big.sum(); // Java: IllegalStateException at run time; Rust: rejected at compile time
  |                      ^^^ value used here after move
...
help: you can `clone` the value and consume it, but this might not be your desired behavior
```

The compiler's suggestion works here, and it's cheap. `big.clone().count()` copies a 16-byte value (a slice iterator
plus a capture-free closure), not the data, and the second traversal re-reads the borrowed `Vec`. The output is
`count=3 total=5750 mean=1916`. A Java stream can't do this, because a stream may be backed by a one-shot source (a
socket, a generator), and the API has to assume the worst. A Rust iterator over *borrowed* data states in its type that
it can be traversed again. An iterator over a one-shot source (a channel receiver, a `vec::IntoIter`) either isn't
`Clone`, or clones its buffered items.

**`?` inside a closure returns from the closure** (listing `ch04-07-question-in-closure.rs`):

```rust,compile_fail
fn total_cents(lines: &[&str]) -> Result<u64, std::num::ParseIntError> {
    let mut total = 0;
    lines.iter().for_each(|l| {
        total += l.parse::<u64>()?; // `?` returns from the CLOSURE, which returns (), not from total_cents
    });
    Ok(total)
}

fn main() {
    println!("{:?}", total_cents(&["1", "2"]));
}
```

```text
error[E0277]: the `?` operator can only be used in a closure that returns `Result` or `Option` (or another type that implements `FromResidual`)
 --> src/main.rs:5:34
  |
4 |     lines.iter().for_each(|l| {
  |                           --- this function should return `Result` or `Option` to accept `?`
5 |         total += l.parse::<u64>()?; // `?` returns from the CLOSURE, which returns (), not from total_cents
  |                                  ^ cannot use the `?` operator in a closure that returns `()`
```

This is the Rust counterpart of "lambdas can't throw checked exceptions," but it's a scoping rule, not a type-system
wall. `?` means "return from the enclosing *function*," and a closure is a function. The fixes use the pipeline instead
of fighting it: `lines.iter().map(|l| l.parse::<u64>()).sum::<Result<u64, _>>()`, or `try_for_each` with a closure
that returns `Result`, or a plain `for` loop, where `?` returns from `total_cents` as intended.

**Stateful operations and Gatherers** (listing `ch04-09-stateful-gatherers.rs`, abridged):

```rust,ignore
    // sorted() + distinct(): in Rust the materialization is explicit (a Vec you can see).
    let mut v = latencies_ms.to_vec();
    v.sort_unstable();
    v.dedup();
    // itertools hides the same buffering behind adapter syntax:
    // latencies_ms.iter().sorted().unique()

    // Gatherers.windowFixed(3)  -> chunks(3) on a slice
    // Gatherers.windowSliding(3) -> windows(3) on a slice (borrowed views, no copies)
    let max_of_3: Vec<u32> = latencies_ms.windows(3).map(|w| *w.iter().max().unwrap()).collect();
    // Gatherers.scan(...)        -> scan: running state, one output per input
    let running: Vec<u32> = latencies_ms.iter().scan(0, |acc, &x| { *acc += x; Some(*acc) }).collect();
```

```text
sorted distinct (Vec):       [7, 9, 12, 30, 55]
sorted().unique() (itertools): [7, 9, 12, 30, 55]
windowFixed(3):   [[12, 7, 12], [30, 7, 55], [9, 30]]
sliding max of 3: [12, 30, 30, 55, 55, 55]
running total:    [12, 19, 31, 61, 68, 123, 132, 162]
within 100 ms:    [12, 19, 31, 61, 68]
deltas:           [-5, 5, 18, -23, 48, -46, 21]
```

Java's `sorted()` and `distinct()` look like ordinary stages but are **barriers**: `sorted()` must buffer the whole
stream before emitting anything. Rust's standard iterators have no `sorted`. You `collect`, then `sort`, and the buffer
is visible in the code. itertools' `sorted()` gives you the Java shape back, and allocates the same `Vec` internally.
[VERSION] Java's **Gatherers** (JEP 485, final in JDK 24 after previews in JDK 22 and 23) add user-defined
intermediate operations such as windows, scans, and folds. Rust has had most of these as ordinary adapters (`scan`,
`map_while`, `take_while`) or slice methods (`chunks`, `windows`), because in Rust a new intermediate operation is just
another struct implementing `Iterator`.

---

## Pass 2 · Systems level — *Two machines under one idea*

### 4. Under the hood

**How a Java stream runs** [LIB, OpenJDK; described from the `java.util.stream` design and source, not verified here].
Each intermediate operation creates a pipeline stage object linked to the previous one (`ReferencePipeline` and its
`StatelessOp`/`StatefulOp` subclasses). Nothing runs until the terminal operation. The terminal operation then asks
each stage to wrap a `Sink` around the downstream sink (`opWrapSink`), producing a chain of sink objects, and calls
`spliterator.forEachRemaining(sink)`: the source **pushes** every element into the chain. Short-circuiting terminals
(`findFirst`, `anyMatch`, `limit`) switch to a pull loop (`tryAdvance` while `!sink.cancellationRequested()`). Stateful
stages like `sorted` buffer everything in `begin`/`accept` and emit it all in `end`.

**How a Rust iterator runs.** Chapter 10.3 showed both paths: `next()` pulls, `fold()`/`try_fold()` pushes. The
difference from Java is *when the shape is decided*. In Java, the sink chain is built at run time, from objects, and
the JIT may or may not inline through it. In Rust, the chain is a type, and both the pull and push paths are
monomorphized and inlined at compile time.

**How `collect::<Result<Vec<T>, E>>()` works** [LIB]. `Result<V, E>` implements `FromIterator<Result<A, E>>` whenever
`V: FromIterator<A>`. std implements it with an internal adapter (`GenericShunt`, not public API) that wraps the input
iterator, yields the `Ok` values to `V::from_iter`, and when it sees an `Err` stores it in a side slot and returns
`None`, which ends the inner collection. Afterwards, if the slot holds an error, the partially built `V` is dropped and
the error is returned. That's why the listing stopped parsing after the bad element: the shunt ended the iteration. The
same machinery serves `Option`, `sum`, and `product`.

**How `FromIterator` targets work.** `collect()` is `FromIterator::from_iter(self)`. `Vec`'s implementation is
specialized (in-place reuse, exact-size pre-allocation, Chapter 10.3). `HashMap`'s inserts each pair in order, so **a
later duplicate key overwrites an earlier one** [LIB]. That's documented behavior, and it's where §10's incident comes
from. `String` collects from `char`, `&str`, and `String`, and `BTreeMap` sorts by key as it goes.

### 5. Memory

**What Java pays that Rust doesn't, by default:**

```text
 Java  List<Long> -> stream().filter().map().mapToLong().sum()
       stage objects (4)          heap; scalar-replaced only if escape analysis succeeds
       lambda objects             non-capturing: cached per call site; capturing: one per evaluation
       Sink objects (per run)     heap, same caveat
       Long elements              ~24 bytes each on a typical 64-bit JVM (12-byte header, the 8-byte value aligned
                                  to offset 16; less with compact object headers), plus a 4-byte compressed
                                  reference in the list; values -128..127 come from the Long.valueOf cache
 Rust  Vec<u64> -> iter().filter().map().sum()
       one stack value (Chapter 10.3), no allocation, 8 bytes per element in one contiguous buffer
```

A Rust-side model of the element and dispatch costs (listing `ch04-10-pipeline-model-bench.rs`: boxed elements model
`Stream<Long>`, and `&dyn Fn` stages model lambda call sites that the JIT couldn't inline; 1,000,000 elements, best of
7, one Playground run, noisy):

```text
static stages, flat u64       (Rust default)   0.41 ns/elem   (result 166167000000)
static stages, boxed elements                  1.16 ns/elem   (result 166167000000)
dyn stages,    flat u64                        1.91 ns/elem   (result 166167000000)
dyn stages,    boxed elements                  1.95 ns/elem   (result 166167000000)
```

This is **a model, not a Java measurement**. The boxes here were allocated in order, so they sit nearly contiguous in
memory (the best case for pointer chasing, Chapter 9.5). The indirect calls always hit the same target, which the
branch predictor learns. The numbers say the two costs are real and of similar size, and that once calls are indirect,
boxing barely adds more (the loop is already waiting on calls). To measure the Java side, write a JMH benchmark over
`List<Long>`, `long[]` with `LongStream`, and a pipeline whose lambdas are passed through a shared helper method from
several call sites (to make them megamorphic): `mvn archetype:generate -DarchetypeGroupId=org.openjdk.jmh
-DarchetypeArtifactId=jmh-java-benchmark-archetype`, then `java -jar target/benchmarks.jar -prof gc` (not verified
here: requires a JDK and Maven).

### 6. CPU / OS

**Parallelism.** Java's `parallelStream()` and Rayon's `par_iter()` have the same shape: split the source recursively,
process the pieces on a work-stealing pool, and combine the results. Listing `ch04-08-rayon.rs` runs a CPU-heavy map
(16 rounds of a hash mixer per element, over 2,000,000 elements), after warming up the pool, best of 5:

```text
threads in Rayon's global pool: 4
sequential  24.77 ms   parallel   6.29 ms   speedup 3.9x
trivial work: sequential  0.19 ms   parallel  0.10 ms
f64 harmonic sum: seq 14.39272672286498889  par 14.39272672286574384  equal: false
```

Three things to take from it:

- **Near-linear speedup for heavy, independent work** (3.9× on 4 threads). The first version of this listing measured
  a *slowdown*, for two reasons worth remembering. The per-item work was a linear recurrence that LLVM folded into
  closed form, so there was nothing to parallelize. And the timing included the pool's thread start-up on first use.
  Benchmarks of parallel code need non-trivial work and a warm pool.
- **Floating-point sums change.** Addition of `f64` isn't associative, and a parallel sum adds in a different order.
  Worse, the order depends on how work was stolen: two runs of this listing produced `...574561` and `...574384`. For
  money, use integers (Meridian's rule since Chapter 2.3). For statistics, decide whether reproducibility or speed
  wins, and document it. [LIB] Java's `DoubleStream.sum()` documents that it may use compensated summation to reduce
  error. Rust's `f64` `Sum` is plain left-to-right addition, and Rayon's is a tree of plain additions.
- **The pool is shared.** Java's parallel streams run on `ForkJoinPool.commonPool()` (by default, parallelism =
  available processors − 1), the same pool that `CompletableFuture`'s async methods use by default, so a blocking call
  inside a parallel stream starves unrelated work. Rayon's global pool is likewise one per process, with one thread per
  core by default. Blocking inside it (I/O, locks held long) starves every other `par_iter` in the process. Both
  ecosystems' answer is a dedicated pool for work with a different profile: `rayon::ThreadPoolBuilder` + `install`,
  or a custom `ForkJoinPool` in Java. Chapter 11.6 covers Rayon's scheduler; Part XIII covers why it doesn't belong on
  async runtime threads.

---

## Pass 3 · Architect level — *Porting without importing Java's defaults*

### 7. Trade-offs

**Error handling in pipelines, by policy:**

| Policy | Rust | Java |
|---|---|---|
| All or nothing, stop at first error | `collect::<Result<Vec<_>, _>>()`, `sum::<Result<_, _>>()`, `try_for_each` | An unchecked exception thrown from a lambda (checked ones wrapped) |
| Skip bad items | `filter_map(Result::ok)` (and count or log what you dropped) | `flatMap` to an empty stream, or `try/catch` returning `null` + `filter(Objects::nonNull)` |
| Keep both | `partition(Result::is_ok)`, or a `fold` into (oks, errs) | `Collectors.partitioningBy` on a wrapper type |
| Stop at a business limit | `try_fold` with `ControlFlow` | `takeWhile` (Java 9) with external state, or a loop |

**When to use what:**

| Situation | Choose |
|---|---|
| Transform/filter/aggregate a collection, one output | An iterator chain |
| One pass producing several outputs, or complex control flow | A `for` loop over an iterator (the review capstone's fix does this) |
| An early exit with a result | `try_fold`, `find_map`, `position` |
| CPU-heavy independent items, large inputs | Rayon's `par_iter` (measure first: tiny work gains little) |
| Grouping | `fold` + entry API, or itertools' `into_group_map` |
| Stateful windows over slices | `windows`, `chunks`, `chunks_exact` (no allocation) |

### 8. Java comparison

The translation table, with the difference that matters in each row:

| Java | Rust | Watch out for |
|---|---|---|
| `list.stream()` | `v.iter()` (borrow) / `v.into_iter()` (consume) | Choose ownership explicitly (Chapter 10.2) |
| `IntStream.range(a, b)` | `a..b` | Ranges are iterators already |
| `filter`, `map`, `flatMap`, `limit`, `skip` | `filter`, `map`, `flat_map`, `take`, `skip` | Same laziness |
| `peek` | `inspect` | Both are for debugging; side effects in them are a smell |
| `mapMulti` (16) | `flat_map` with a small iterator, or a `fold` | |
| `takeWhile`/`dropWhile` (9) | `take_while`/`skip_while` | |
| `sorted()`, `distinct()` | `collect` + `sort`/`dedup`, or `HashSet`; itertools `sorted`/`unique` | Rust makes the barrier's buffer visible |
| `sorted(comparator)` stability | `sort` (stable) vs `sort_unstable` | Java's `sorted()` is stable for ordered streams; pick deliberately in Rust |
| `forEach` | `for_each` or a `for` loop | Prefer `for` when you need `?`, `break`, `return` |
| `reduce(identity, op)` | `fold(identity, op)` / `reduce(op)` | Rust's `reduce` returns `Option` (empty input) |
| `count()` | `count()` | Rust's consumes; Java's may skip execution for sized streams (Java 9+), so `peek` side effects may not run |
| `anyMatch`/`allMatch`/`noneMatch` | `any`/`all`/`!any` | Both short-circuit |
| `findFirst()` → `Optional<T>` | `next()` / `find(p)` → `Option<T>` | `Option<&T>` borrows; `Optional` holds a reference |
| `toList()` (16) | `collect::<Vec<_>>()` | Java's is unmodifiable; a Rust `Vec` is yours |
| `Collectors.toMap(k, v)` | `collect::<HashMap<_, _>>()` | **Java throws on duplicate keys; Rust keeps the last value** (§10) |
| `Collectors.toMap(k, v, merge)` | `fold` + `entry().and_modify().or_insert()` | |
| `Collectors.groupingBy(k)` | `fold` into `HashMap<K, Vec<V>>` / itertools `into_group_map` | `groupingBy` returns an unordered `HashMap`; so does Rust's. Use `BTreeMap`/`TreeMap` for stable output |
| `Collectors.partitioningBy` | `partition` | Rust returns two collections, not a `Map<Boolean, _>` |
| `Collectors.joining(", ")` | `collect::<Vec<_>>().join(", ")`, itertools `join` | |
| `parallelStream()` | Rayon `par_iter()` | Shared pools on both sides; float results differ between runs |
| Reusing a stream | `clone()` a borrowing iterator | Java fails at run time; Rust at compile time |
| Checked exceptions in lambdas | `Result` items + `collect`/`try_fold`/`?` in a loop | `?` in a closure returns from the closure |

> **Analogy limit.** "Iterators are Rust's streams" holds for the vocabulary and for laziness. It fails for the
> **defaults**. Java's defaults are boxing, heap pipelines, run-time optimization, exceptions beside the data, and
> duplicate keys that fail loudly. Rust's defaults are unboxed values, stack pipelines, compile-time optimization,
> errors as data, and duplicate keys that fail silently. A literal port keeps Java's intent but inherits Rust's
> defaults. Read every `collect` in a ported file with the question "what did the Java version do here?"

### 9. Production scenario

**Porting Meridian's merchant statement job.** The ledger team (whose core stays Java, Chapter 1.2) moved the monthly
merchant statement renderer to Rust, next to the settlement batch job of Chapter 10.3. The Java core of the
aggregation was:

```java
// Java (illustrative, not verified here)
Map<String, LongSummaryStatistics> byMerchant = txns.stream()
    .filter(t -> t.status() == SETTLED)
    .collect(Collectors.groupingBy(Txn::merchant, Collectors.summarizingLong(Txn::cents)));
```

The Rust port kept the shape and changed three decisions, each written in the PR description:

1. **`BTreeMap`, not `HashMap`.** The Java `groupingBy` returned a `HashMap`, and the renderer sorted the keys later.
   Two statements from the same data came out in different orders in a test, because Rust's `HashMap` iteration order
   is randomized per process (SipHash with random keys, Chapter 9.3), while Java's `HashMap` order, unspecified but
   stable in practice for the same keys, had hidden the missing sort for years.
2. **A `fold` into an explicit accumulator** (`count`, `sum`, `min`, `max` as `u64`s), not `f64` statistics.
   `summarizingLong` was integer-exact, and the port had to stay exact.
3. **Errors as items.** Rows come from a cursor as `Result<Txn, LedgerError>`, and the aggregation is a `try_fold`. A
   corrupt row aborts the merchant's statement with an error that names the row, instead of Java's
   `RuntimeException` from inside a lambda, which had surfaced in logs as a stack trace through
   `ReferencePipeline$3$1.accept`.

### 10. Failure scenario

**The fee schedule that stopped failing.** Meridian's merchant fee schedule arrives nightly as a CSV export from a
partner billing system: one row per merchant, with a fee in basis points. The Java loader was:

```java
// Java (illustrative, not verified here)
Map<String, Integer> fees = rows.stream()
    .collect(Collectors.toMap(Row::merchant, Row::bps));   // IllegalStateException: Duplicate key m-100 ...
```

When the partner's export once repeated a merchant, with a promotional rate on the second row, the Java loader threw
at startup, the job failed loudly, and the partner fixed the file. The Rust port (listing
`ch04-04-tomap-duplicates.rs`) was a faithful translation of the *code*:

```rust,ignore
/// The literal port: compiles, runs, and silently picks 190 bps for m-100.
fn load_ported(rows: Vec<(&'static str, u32)>) -> HashMap<&'static str, u32> {
    rows.into_iter().collect()
}
```

```text
ported: m-100 -> 190 bps (3 merchants)
strict: Err("duplicate key m-100: 290 and 190")
```

The next time the export repeated a merchant, nothing failed. The last row won, and merchant m-100 was charged the
promotional 190 bps instead of 290 for three days, until the monthly reconciliation of fees against contracts flagged
the difference. The Java code had carried a business rule, "a duplicate is an error," inside a library default, and
the port replaced the default.

The strict loader restores the rule explicitly (excerpt of listing `ch04-04-tomap-duplicates.rs`):

```rust,ignore
/// The faithful port: duplicates are an error, like toMap without a merge function.
fn load_strict(rows: Vec<(&'static str, u32)>) -> Result<HashMap<&'static str, u32>, String> {
    rows.into_iter().try_fold(HashMap::new(), |mut m, (k, v)| match m.entry(k) {
        Entry::Vacant(e) => {
            e.insert(v);
            Ok(m)
        }
        Entry::Occupied(e) => Err(format!("duplicate key {k}: {} and {v}", e.get())),
    })
}
```

The team's porting checklist now has a row for every `Collectors` method whose Rust equivalent has different failure
semantics (`toMap`, `toUnmodifiableMap`, `toSet` of mutable elements). Each port gets a test that feeds the
problematic input (a duplicate key) and asserts the Java behavior.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part X).*

1. Java streams push elements through a sink chain; Rust iterators are pulled with `next()`. Is that the whole story?
   Where does each side use the other model?
2. How does `collect::<Result<Vec<_>, _>>()` stop at the first error? What happens to the partially built `Vec`?
3. Why can't a Java lambda throw a checked exception, and what is the Rust equivalent of that restriction? Why does `?`
   inside a `for_each` closure fail to compile?
4. What does `Collectors.toMap` do with a duplicate key, and what does `collect::<HashMap<_, _>>()` do? How do you port
   the Java behavior?
5. Why can a Rust iterator be traversed twice via `clone()`, when a Java stream can't be reused at all?
6. What do `sorted()` and `distinct()` cost in each language, and why does Rust's standard library not have `sorted`
   as an adapter?
7. What are the costs of `Stream<Long>` compared with `Vec<u64>` iteration? Which of them can the JIT remove, and when?
8. Why can a parallel sum of `f64` give different results between runs? What do you do about it for money, and for
   metrics?
9. What's the risk of running blocking work inside `parallelStream()` or Rayon's global pool?

### 12. Exercises

- **Beginner.** Translate: `names.stream().filter(n -> n.length() > 3).map(String::toUpperCase).sorted().toList()`.
  Borrow the input. What's the output type, and who owns the strings?
- **Intermediate.** Implement `groupingBy(k, counting())` and `groupingBy(k, mapping(f, toList()))` as generic helper
  functions over iterators. Then compare with itertools' `counts` and `into_group_map`.
- **Advanced.** Implement `FromIterator<(K, V)>` for a `StrictMap<K, V>` that records duplicates instead of
  overwriting, and then expose `into_result() -> Result<HashMap<K, V>, Vec<K>>`. Why can't `FromIterator` itself
  return a `Result`?
- **Systems.** Run listing `ch04-08-rayon.rs` five times and record the `f64` sums. Then replace the parallel `sum`
  with a Kahan-summation `fold` + `reduce`. Does it become deterministic? Why or why not?
- **Architecture.** Your team is porting 30,000 lines of stream-heavy Java to Rust. Write the porting checklist:
  which Java stream idioms must never be translated literally, and what each must be checked against.

### 13. Debugging exercise

```rust,ignore
fn total_cents(lines: &[&str]) -> Result<u64, std::num::ParseIntError> {
    let mut total = 0;
    lines.iter().for_each(|l| {
        total += l.parse::<u64>()?; // `?` returns from the CLOSURE, which returns (), not from total_cents
    });
    Ok(total)
}
```

1. Predict the error (listing `ch04-07`). Which function does the compiler say "should return `Result` or `Option`"?
2. Fix it three ways: with `try_for_each`, with `map` + `sum::<Result<..>>()`, and with a `for` loop. Which one would you
   ship, and why?
3. A teammate "fixes" it with `l.parse::<u64>().unwrap_or(0)`. Describe the production incident that fix eventually
   causes.

### 14. Design exercise

**Parallelizing Meridian's nightly risk recomputation.** The risk-limits service (Chapter 1.2) recomputes exposure for
3 million merchants every night from the day's transactions (about 40M rows). Per merchant, the work is a fold over
that merchant's transactions, with no cross-merchant dependencies. The service also serves online limit checks from
the same process (p99 < 1 ms).

Design the parallel job:

- Where the data comes from and how it's partitioned by merchant before parallel work begins.
- Rayon's global pool or a dedicated pool, and how many threads, given that the online path shares the machine.
- How results stay deterministic (integer arithmetic, ordered output), and how you'd test that two runs agree.
- What one merchant's corrupt row does to the whole job (fail the job, skip the merchant, or quarantine), expressed as
  the pipeline's error policy from §7.
