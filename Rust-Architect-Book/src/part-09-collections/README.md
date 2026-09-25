# Part IX — Collections and Memory

> **Part question:** *For each collection in std, where do its bytes live, when does it allocate, what does each
> operation pull through the cache hierarchy, and how do you choose among them for a workload you can describe?*

Parts III and IV established who owns data and who may touch it. Part IX is about the containers that hold it, and it
treats them the way a systems architect should: as **memory layouts with costs attached**, not as abstract data types
with a Big-O column. Every structure gets the same questions: memory layout, growth strategy, allocation behavior,
cache locality, Big-O, constant factors, ownership behavior, and concurrency considerations.

Almost every claim here is measured: allocations with the counting allocator (exact), times from single release runs
on a shared machine (noisy, labelled, used for ratios), and compiler output fetched from the Playground (the assembly
of `Vec::push`, a recursive function's 96-byte frame, and the stack probes that make overflow a clean abort). Where a
number is implementation behavior rather than a guarantee, it's tagged [LIB]: `Vec`'s doubling, `HashMap`'s 7/8 load
factor, `BTreeMap`'s 11-key nodes.

The Part ends with an **interlude** that is also a template: BFS vs DFS analyzed the way the SPEC asks every trade-off
to be analyzed, from the algorithm down to the guard page under a thread's stack, and summarized in a twelve-criterion
decision matrix.

## Chapter map

```text
9.1 Vec<T>: Pointer, Length, Capacity    (cap, ptr, len) in the asm; the growth rule [LIB] and its minimums;
                                         reserve/shrink/retain/collect costs; arrays, slices, Box<[T]>, SmallVec;
                                         glibc realloc and mremap; the buffer that never shrank
      │
9.2 String and Text Encoding             building text (format! vs write!), formatting internals, UTF-16 and
                                         Windows/Java boundaries (Modified UTF-8), case-mapping costs, String vs
                                         Box<str> vs Arc<str>, small-string types; memchr and when skipping pays
      │
9.3 HashMap and HashSet                  SwissTable: control bytes, h1/h2, SIMD groups; growth and rehash counts;
                                         entry vs get_mut+insert (hashes AND allocations); hashers and HashDoS
                                         (a measured quadratic blowup); f64 keys; memory per entry at 2M entries
      │
9.4 VecDeque, BTreeMap, BinaryHeap —     ring buffers; B-trees with 11-key nodes; an order book by price level;
    and Why Not LinkedList               top-k and EDF; LinkedList measured (4.7× to 47× slower); cursors unstable
      │
9.5 Choosing Collections by Cache        bytes touched, dependent loads, predictability; AoS vs SoA; boxes as
    Behavior                             Java's memory model; lookup structures by size; a selection method
      │
Interlude: The Trade-off Engine —        frame size measured (224 B debug / 96 B release); overflow verified at 110%
BFS vs DFS, Down to the Stack Page       of prediction; guard pages and stack probes; explicit-stack DFS that keeps
                                         DFS semantics; BFS frontiers; stack sizes by platform; the decision matrix
      │
Part IX Review                           a collections design review of a Java-shaped port; memory budget; interview mode
```

## What you'll be able to do after Part IX

- Predict the allocations and the peak memory of a collection-heavy function before running it, and verify the
  prediction with a counting allocator.
- Choose a collection from its access pattern, its size relative to cache, and the density of its keys, and justify
  the choice with a decision matrix rather than Big-O alone.
- Choose a hasher as a security decision, and recognize a HashDoS-shaped profile.
- Build text and move it across encoding boundaries without hidden allocations or silent corruption.
- Convert recursion into an explicit stack that preserves semantics, and predict when recursion will overflow a given
  stack on a given platform.

## Listings

`listings/part-09/`: 32 files, 40 checks, all verified on rustc 1.98.1 (edition 2024), including two verified
stack-overflow aborts (debug and release) and three compiler-artifact listings inspected with `tools/emit.ps1`.
Timing listings print one run's numbers. They're quoted as measured, and they will differ on your machine; the ratios
and the scaling are what the chapters rely on.
