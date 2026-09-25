# Chapter 10.3 — How an Iterator Chain Compiles

> **Where this sits:** Part X · Closures, Iterators, and Zero-Cost Abstractions · chapter 3 of 4
> **Prerequisites:** Chapter 1.3 (the verified "loop == chain" assembly), Chapter 7.1 (22 debug functions become 2),
> Chapters 10.1–10.2.
> **After this chapter you can:** explain, adapter by adapter, how a chain becomes one loop; predict when the fold path
> beats a `for` loop; say exactly what `Box<dyn Iterator>` costs and why; see where bounds checks survive; count the
> allocations a pipeline makes; and explain in-place `collect`, including the memory it can quietly keep.

---

## Pass 1 · User level — *The chain is a value*

### 1. Problem

Chapter 1.3 showed that `data.iter().filter(..).map(..).sum()` and a hand-written index loop compile to the same
assembly. Chapter 7.1 showed half of the reason: the debug build has 22 functions (`filter`, `map`, `sum`, `fold`, two
closures, and their helpers), and the release build has 2. Monomorphization made every call static, and inlining
removed them.

That explanation is true but incomplete. It doesn't say what the adapters *are*, why `sum()` sometimes runs a
different loop structure from a `for` loop over the same chain, what happens when a `dyn` gets in the way, or why a
pipeline that allocates nothing in its adapters can still keep a megabyte you didn't ask for. This chapter takes the
chain apart and puts a measurement next to each claim, because "zero-cost" is a property of specific code shapes, not
of the word `iter`.

### 2. Mental model

**Four ingredients make a chain as fast as a loop:**

```text
 1. ADAPTERS ARE STRUCTS       .filter(p) returns Filter<I, P>: the previous iterator + the closure, by value.
                               The whole pipeline is ONE nested value, usually on the stack.
 2. MONOMORPHIZATION           Filter<Iter<u64>, {closure#0}>::next is a separate function for THIS closure type,
                               so every call inside it is a direct call to a known function (Chapter 7.1).
 3. INLINING                   LLVM inlines those small direct calls until next() of the outermost adapter
                               contains the whole pipeline: one loop, no calls, closures dissolved into it.
 4. INTERNAL ITERATION         consumers like sum/fold/for_each call fold() instead of next(); each adapter
                               overrides fold() to run its OWN best loop shape (Chain: two loops, not one loop
                               that checks "which half am I in?" per element).
 ... then ordinary loop optimization: bounds-check elimination, unrolling, vectorization.
```

Remove any ingredient and the cost comes back. Without monomorphization (a `dyn Iterator`), calls stay indirect.
Without inlining (a debug build), every `next()` is a real call. Without internal iteration (a `for` loop over a
`Chain`), the loop carries a state check per element.

**"Iterator fusion" has two meanings in Rust, so keep them apart.** *Loop fusion* (several passes over the data become
one) is automatic by construction: a lazy pipeline is already one pass. *Fusing* an iterator (`fuse()`,
`FusedIterator`, Chapter 10.2) means making `None` final. They're unrelated.

### 3. Rust code

**The pipeline as a type** (listing `ch03-01-adapter-types.rs`; type names shortened by removing module paths):

```rust
// Each adapter wraps the previous iterator in a new struct. The pipeline is one nested value on the stack.
use std::any::type_name_of_val;
use std::mem::size_of_val;

fn show<T>(label: &str, it: &T) {
    let name = type_name_of_val(it).replace("core::iter::adapters::", "").replace("core::slice::iter::", "");
    println!("{label:<12} {:>2} B  {name}", size_of_val(it));
}

fn main() {
    let amounts: Vec<u64> = vec![120, 5, 980, 42, 3_000];
    let fee_bps = 290_u64;

    let s0 = amounts.iter();
    show("iter()", &s0);
    let s1 = s0.filter(|a| **a >= 100); // closure captures nothing: 0 bytes
    show(".filter()", &s1);
    let s2 = s1.map(|a| a * fee_bps / 10_000); // closure captures &fee_bps: 8 bytes
    show(".map()", &s2);
    let s3 = s2.take(2); // adds a counter
    show(".take(2)", &s3);
    let s4 = s3.enumerate(); // adds another counter
    show(".enumerate()", &s4);

    let fees: Vec<(usize, u64)> = s4.collect();
    println!("result: {fees:?}");
}
```

```text
iter()       16 B  Iter<'_, u64>
.filter()    16 B  filter::Filter<Iter<'_, u64>, playground::main::{{closure}}>
.map()       24 B  map::Map<filter::Filter<Iter<'_, u64>, playground::main::{{closure}}>, playground::main::{{closure}}>
.take(2)     32 B  take::Take<map::Map<filter::Filter<Iter<'_, u64>, playground::main::{{closure}}>, playground::main::{{closure}}>>
.enumerate() 40 B  enumerate::Enumerate<take::Take<map::Map<filter::Filter<Iter<'_, u64>, playground::main::{{closure}}>, playground::main::{{closure}}>>>
result: [(0, 3), (1, 28)]
```

The type *is* the pipeline, like an AST of the computation encoded in generics. The sizes add up field by field:
16 bytes of slice iterator, 0 for a capture-free closure, 8 for a closure holding `&fee_bps`, and 8 per counter. It's
stack data. Building a pipeline allocates nothing.

**What the adapters do** (listing `ch03-02-hand-rolled-adapters.rs`). `Filter` and `Map`, written the way std writes
them [LIB]:

```rust,ignore
impl<I: Iterator, P: FnMut(&I::Item) -> bool> Iterator for MyFilter<I, P> {
    type Item = I::Item;
    fn next(&mut self) -> Option<I::Item> {
        // std: `self.iter.find(&mut self.predicate)`
        while let Some(x) = self.iter.next() {
            if (self.pred)(&x) {
                return Some(x);
            }
        }
        None
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, self.iter.size_hint().1) // might reject everything, can't add anything
    }
}

impl<B, I: Iterator, F: FnMut(I::Item) -> B> Iterator for MyMap<I, F> {
    type Item = B;
    fn next(&mut self) -> Option<B> {
        // std: `self.iter.next().map(&mut self.f)`
        self.iter.next().map(&mut self.f)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint() // one out per one in
    }
}
```

```text
hand-rolled: [3, 28, 87]
std:         [3, 28, 87]
```

That's all an adapter is: a struct, a `next` that calls the inner `next`, and a closure call. After monomorphization,
`MyMap<MyFilter<Iter<u64>, P>, F>::next` calls `MyFilter<Iter<u64>, P>::next`, which calls `Iter<u64>::next` and `P`,
all known functions. After inlining, those calls are gone.

---

## Pass 2 · Systems level — *Two paths through every chain*

### 4. Under the hood

**`next()` versus `fold()`.** Chapter 7.1's list of the debug build's functions contained something a `for` loop would
never produce: `sum` calls `fold`, and `Filter`'s `fold` wraps the accumulator closure in `filter_fold`, then calls the
inner iterator's `fold`. That's **internal iteration**: instead of the consumer pulling items one at a time through
`next()`, the consumer hands a closure to the outermost adapter, and each adapter passes it inward, wrapped. The
source then runs a plain loop and pushes each item through the stack of wrapped closures.

Every consumer that doesn't need to stop early (`sum`, `count`, `fold`, `for_each`, `collect` into most collections,
`max`, `last`) goes through `fold` or `try_fold` [LIB]. Every adapter can override them. For most adapters the result
after inlining is the same loop either way. For adapters with internal state, it isn't.

**`Chain`, both ways.** `a.iter().chain(b.iter())` must yield all of `a`, then all of `b`. Through `next()`, every call
has to ask "am I still in `a`?". Through `fold()`, `Chain` simply folds `a`, then folds `b` [LIB]. Listing
`ch03-03-internal-external-asm.rs` compiles both (release, rustc 1.98.1, labels simplified):

```rust,ignore
#[inline(never)]
pub fn chain_sum_external(a: &[u64], b: &[u64]) -> u64 {
    let mut total = 0;
    for x in a.iter().chain(b.iter()) {
        total += *x;
    }
    total
}

#[inline(never)]
pub fn chain_sum_internal(a: &[u64], b: &[u64]) -> u64 {
    a.iter().chain(b.iter()).sum()
}
```

```text
playground::chain_sum_external:            ; the for loop (next)
	...
.LBB0_3:
	add	rax, qword ptr [rdi]               ; total += *x          one element per iteration
	add	rdi, 8
	test	rdi, rdi                       ; is the first half still active? (Option<Iter> niche: null = None)
	je	.LBB0_4
.LBB0_2:
	cmp	rdi, rsi                           ; end of the first half?
	jne	.LBB0_3
.LBB0_4:
	cmp	rdx, rcx                           ; second half: one element at a time too
	je	.LBB0_6
	add	rax, qword ptr [rdx]
	add	rdx, 8
	xor	edi, edi                           ; first half := None
	test	rdi, rdi
	jne	.LBB0_2
	jmp	.LBB0_4

playground::chain_sum_internal:            ; sum() (fold): two independent, vectorized loops
	...
.LBB1_6:                                   ; first half: 4 u64 per iteration, two SSE accumulators
	movdqu	xmm2, xmmword ptr [rdi + r9]
	paddq	xmm1, xmm2
	movdqu	xmm2, xmmword ptr [rdi + r9 + 16]
	paddq	xmm0, xmm2
	add	r9, 32
	cmp	rax, r9
	jne	.LBB1_6
	...
.LBB1_14:                                  ; second half: the same loop again
	movdqu	xmm2, xmmword ptr [rdx + rdi]
	paddq	xmm0, xmm2
	movdqu	xmm2, xmmword ptr [rdx + rdi + 16]
	paddq	xmm1, xmm2
	add	rdi, 32
	cmp	rax, rdi
	jne	.LBB1_14
```

The external version is a scalar loop that re-tests the chain's state on every element, and LLVM didn't vectorize it.
The internal version is two textbook vectorized sums (`paddq`, four `u64` per iteration). Same chain, same
optimization level, different consumer. The `test rdi, rdi` is `Chain`'s `Option<Iter>` niche check (Chapter 5.2):
a null pointer means the first half is finished.

**`Box<dyn Iterator>`: only `next()` crosses the boundary.** A `dyn Iterator` vtable can only contain methods that are
dyn-compatible (Chapter 6.4). `fold`, `map`, and `sum` are generic, so they're not in it. The vtable holds `next`,
`size_hint`, `nth`, and friends. So `sum()` over a `Box<dyn Iterator>` runs the *default* `fold`, which is a loop over
`next()`, through the vtable, per element (excerpt of listing `ch03-03-internal-external-asm.rs`):

```rust,ignore
#[inline(never)]
pub fn dyn_sum(it: &mut dyn Iterator<Item = u64>) -> u64 {
    let mut total = 0;
    for x in it {
        total += x;
    }
    total
}
```

```text
playground::dyn_sum:
	...
	mov	r15, qword ptr [rsi + 24]          ; vtable slot 3: Iterator::next (after drop, size, align)
	call	r15                            ; first next()
	...
.LBB2_1:
	add	r14, rdx                           ; Option<u64> comes back in rax (tag) : rdx (value)
	mov	rdi, rbx
	call	r15                            ; next(), per element
	test	al, 1
	jne	.LBB2_1
```

Every element costs an indirect call, an `Option` returned in two registers, and a tag test. Nothing inside the
iterator can be inlined into the loop, and nothing in the loop can be vectorized.

### 5. Memory

**Intermediate collections are where pipelines allocate.** Listing `ch03-07-intermediate-collect.rs` computes the same
fee total over 100,000 transactions twice, once the way a literal port of Java stream code often looks (a `collect()`
after each step), and once fused:

```rust,ignore
    // Java-stream habit ported literally: each step collects.
    let (total_a, allocs_a, bytes_a) = counting::measure(|| {
        let settled: Vec<&Txn> = txns.iter().filter(|t| t.settled).collect();
        let big: Vec<&Txn> = settled.into_iter().filter(|t| t.amount_cents >= 10_000).collect();
        let fees: Vec<u64> = big.iter().map(|t| t.amount_cents * 290 / 10_000).collect();
        fees.iter().sum::<u64>()
    });
    // One fused pipeline.
    let (total_b, allocs_b, bytes_b) = counting::measure(|| {
        txns.iter()
            .filter(|t| t.settled && t.amount_cents >= 10_000)
            .map(|t| t.amount_cents * 290 / 10_000)
            .sum::<u64>()
    });
```

```text
collect per stage: total=62604000 allocations=17 bytes=2673120
fused pipeline:    total=62604000 allocations=0 bytes=0
```

Seventeen allocations and 2.67 MB of intermediate vectors, against none. The numbers account for themselves exactly.
The first `collect` (90,000 settled references) grows by doubling from capacity 4 to 131,072, because `filter` gives
`collect` no useful lower bound. That's 16 allocations totaling 2,097,120 bytes. The third, `fees`, knows its exact
length from `map` (72,000 × 8 = 576,000 bytes, one allocation). 16 + 1 = 17, and 2,097,120 + 576,000 = 2,673,120. So
the second `collect` allocated **nothing**: it reused the first vector's buffer. That's the next topic.

**In-place `collect`** [LIB]. When the source is a `vec::IntoIter` (a `Vec` consumed by `into_iter()`) and the result
is collected into a `Vec`, std can write the output into the **source's own buffer**, because the adapters read each
element before the output overwrites its slot. Listing `ch03-06-in-place-collect.rs` measures it with the counting
allocator ("same buffer" means the output's pointer equals the source's; identical output in debug and release):

```text
u64 -> u64 via map                       allocs=0  same buffer=true   len=1000000 cap=1000000 (source cap 1000000)
u64 -> u64 via filter (keeps 1%)         allocs=0  same buffer=true   len=10000   cap=1000000 (source cap 1000000)
u64 -> u32 via map                       allocs=1  same buffer=false  len=1000000 cap=1000000 (source cap 1000000)
(u32, u32) -> u32 via map                allocs=0  same buffer=true   len=1000000 cap=2000000 (source cap 1000000)
u32 -> u64 via map (grows)               allocs=1  same buffer=false  len=1000000 cap=1000000 (source cap 1000000)
String -> usize via map (len)            allocs=0  same buffer=true   len=1000    cap=3000    (source cap 1000)
u64 -> u64 via filter, shrink_to_fit     allocs=1  same buffer=false  len=10000   cap=10000   (source cap 1000000)
```

Reading the table as rules (observed on rustc 1.98.1; these are std implementation choices, not guarantees):

- **Same size, same alignment**: always in place, zero allocations.
- **Smaller output with the same alignment** (`(u32, u32)` → `u32`, or `String` → `usize`): in place, and the
  capacity is **re-expressed in output elements**. The buffer that held 1,000,000 pairs (8 MB) now has room for
  2,000,000 `u32`s, and 1,000 `String`s' 24 KB becomes capacity for 3,000 `usize`s.
- **Different alignment** (`u64` → `u32`) or a **larger output** (`u32` → `u64`): a fresh allocation.
- **`filter` in place keeps the whole buffer.** Keep 1% of a million and you still hold a million slots.

How it's wired [LIB]: `Vec`'s `FromIterator` is specialized. When the iterator is a chain of adapters over a
`vec::IntoIter`, std reaches the source through the chain via an internal `SourceIter` trait and checks that every
adapter is `InPlaceIterable`, meaning it never yields more items than it has consumed, so the write position can't
overtake the read position. Then it runs the pipeline with `try_fold`, writing each output at the front of the source
buffer, and finally takes the buffer over as the new `Vec`. Both traits are unstable internals of std. At the time of
writing, the documentation of `Vec`'s `FromIterator` impl says the source allocation may be reused and points to
`shrink_to_fit` (or `into_boxed_slice`) for when the excess capacity matters. The last row shows what that costs: one
allocation and a copy of the kept elements.

### 6. CPU / OS

**Bounds checks.** The folklore says iterators are faster than indexing because indexing is checked. Listing
`ch03-04-bounds-checks-asm.rs` compiles `out[i] = a[i] + b[i]` three ways and shows it's more interesting than that
(release, trimmed):

```rust,ignore
#[inline(never)]
pub fn add_indexed(out: &mut [u64], a: &[u64], b: &[u64]) {
    for i in 0..out.len() {
        out[i] = a[i] + b[i]; // a and b may be shorter than out: each access must be checked
    }
}

#[inline(never)]
pub fn add_resliced(out: &mut [u64], a: &[u64], b: &[u64]) {
    let n = out.len();
    let (a, b) = (&a[..n], &b[..n]); // check once, up front (panics if too short)
    for i in 0..n {
        out[i] = a[i] + b[i]; // now provably in bounds
    }
}

#[inline(never)]
pub fn add_zipped(out: &mut [u64], a: &[u64], b: &[u64]) {
    for ((o, x), y) in out.iter_mut().zip(a).zip(b) {
        *o = x + y; // stops at the shortest slice: no checks needed
    }
}
```

```text
playground::add_indexed:
	...                                    ; computes min(len_a, len_b, n-1): the provably safe prefix
.LBB1_8:                                   ; vectorized main loop over that prefix: no checks
	movdqu	xmm0, xmmword ptr [rdx + 8*r10]
	...
	paddq	xmm2, xmm0
	...
	add	r10, 4
	cmp	rax, r10
	jne	.LBB1_8
.LBB1_4:                                   ; scalar tail: TWO bounds checks per element
	cmp	r11, rax
	je	.LBB1_9                            ; -> panic_bounds_check (a too short)
	cmp	r10, rax
	je	.LBB1_6                            ; -> panic_bounds_check (b too short)
	mov	rbx, qword ptr [r8 + 8*rax]
	add	rbx, qword ptr [rdx + 8*rax]
	...

playground::add_resliced:
	cmp	rsi, rcx
	ja	.LBB2_10                           ; -> slice_index_fail: checked ONCE, before the loop
	cmp	rsi, r9
	ja	.LBB2_11
	...                                    ; then the same vectorized loop, no checks inside

playground::add_zipped:
	cmp	rcx, rsi
	cmovb	rsi, rcx                       ; n = min(len_out, len_a, len_b)
	cmp	r9, rsi
	cmovb	rsi, r9
	...                                    ; the same vectorized loop, no checks, no panic paths
```

All three are vectorized. LLVM split the indexed loop into a prefix it could prove safe (vectorized, unchecked) and a
scalar tail that keeps the checks, whose only job is to reach the panic at the right index. So the speed difference is
small here. The real difference is **semantics**: `add_indexed` writes part of `out` and then panics if `a` is short;
`add_resliced` panics before writing anything; `add_zipped` silently processes the shortest length. Choose the one
that states your intent. `zip` is right when "stop at the shortest" is correct, reslicing when unequal lengths are a
bug. LLVM usually handles the rest, as long as the loop is simple enough to analyze.

**The measurements** (listing `ch03-05-zero-cost-bench.rs`: 1,000,000 `u64`s, best of 7 runs, one Playground run,
noisy; results repeated within ±0.03 ns on a second run):

| Variant | Release ns/elem | Debug ns/elem |
|---|---|---|
| index loop (even squares) | 0.41 | 15.28 |
| iterator chain (even squares) | 0.41 | 10.24 |
| `chain()`: `for` loop (next) | 0.41 | 10.79 |
| `chain()`: `sum` (fold) | **0.14** | 5.20 |
| `Box<dyn Iterator>`: sum | **1.63** | 7.39 |
| `Box<dyn Iterator>`: sum, type visible | 0.08 | 7.43 |
| `Vec<Vec>` (250,000 × 4): `flatten().sum()` | 0.75 | 10.73 |
| `Vec<Vec>`: `for` over `flatten()` (next) | 0.52 | 14.32 |
| `Vec<Vec>`: nested `for` loops | 0.75 | 6.71 |
| `collect()` mid-pipeline, then sum | 0.81 | 24.58 |

What this table says, and what it doesn't:

- **Chain = loop, measured.** 0.41 = 0.41 ns, the runtime counterpart of Chapter 1.3's identical assembly.
- **The consumer matters.** The same `chain` costs 0.41 ns through `next` and 0.14 through `fold`, 3×, exactly the
  vectorized-versus-scalar difference in the assembly above.
- **`dyn` costs ~20× on a tight loop** (1.63 vs 0.08), and it's the lost optimization, not the call. When the concrete
  type was visible in the same function, LLVM **devirtualized** the box entirely (0.08 ns: a vectorized sum). The first
  version of this benchmark measured only that case by accident. `black_box` hides the type, which is what a real
  plugin boundary does.
- **Flatten over tiny inner vectors isn't the villain folklore makes it.** Here, the `next` path was the fastest of the
  three. With 4 elements per inner `Vec`, the per-inner-vector overhead and the pointer chase to each `Vec`'s buffer
  dominate whatever the loop shape is. I don't have an explanation for the ordering that I've verified, so treat it as
  a measured fact about this shape on this machine, not a rule.
- **Debug builds invert everything**, and not uniformly. The chain is 25× slower in debug, the index loop 37×, and
  `collect` mid-pipeline 30×. Nothing in debug timings predicts release timings. Never profile a debug build.

---

## Pass 3 · Architect level — *Where "zero-cost" is a promise, and where it's a hope*

### 7. Trade-offs

| Code shape | Release cost vs a hand-written loop | Why |
|---|---|---|
| Static chain over slices/ranges, any consumer | Same | Monomorphization + inlining + loop optimizations |
| `Chain` / `Flatten` / `FlatMap` with `for` | Often worse than `fold`-based consumers | Per-element state checks in `next()` |
| Same, with `sum`/`fold`/`for_each`/`try_for_each` | Same or better | Each adapter runs its own loop shape |
| `Box<dyn Iterator>` or `&mut dyn Iterator` in a hot loop | ~20× on tight arithmetic (measured) | Indirect `next()` per item; nothing inlines across it |
| `collect()` in the middle | Allocations + extra passes | Materialized intermediate stages |
| `into_iter().filter(..).collect()` into a long-lived `Vec` | Same speed, **more memory** | In-place collect keeps the source capacity |
| Any iterator code in a debug build | 10–40× slower (measured) | No inlining |
| Closure captures a large value by `move` | A copy per closure construction | The closure is a struct (Chapter 10.1) |

The rules that fall out:

1. **Prefer internal-iteration consumers** (`sum`, `fold`, `for_each`, `try_for_each`, `extend`) when the chain
   contains `chain`, `flatten`, `flat_map`, or `skip_while`. Use `for` when the body needs `break`, `return`, or `?`
   (or use `try_for_each`, which gives you both).
2. **Put `dyn` at coarse boundaries**: per batch, per file, per request. Not per element. A `Box<dyn Iterator>` that
   yields *batches* costs one indirect call per batch.
3. **Don't `collect()` to "make it simpler"** unless you need the collection (multiple passes, random access, sorting,
   or an owner that outlives the source).
4. **After filtering an owned `Vec` into a value that lives long, `shrink_to_fit()`**, or build it from a borrowed
   iterator instead.

> **Why not just write loops everywhere, to be safe?** Because loops don't win either. The fastest row in the table is
> a `fold`-based consumer, which is harder to hand-write (you'd write two loops for `chain` yourself). The index loop
> tied the chain and carried bounds-check semantics you had to think about. Iterators are the better default, and the
> rules above are about the specific shapes that aren't free.

### 8. Java comparison

HotSpot can do much of the same. When a stream pipeline's lambdas are inlined into one compiled method, C2 applies
**escape analysis** to the pipeline objects (so they needn't be allocated), **range-check elimination** to array
accesses, and **superword** vectorization to simple loops. A well-warmed, monomorphic stream can come close to a
hand-written loop.

The difference is in the conditions:

| | Rust | Java (HotSpot C2) |
|---|---|---|
| When optimization happens | Compile time, every build, deterministically | At run time, after warm-up, based on profiles |
| What enables inlining | The types: every call in the chain is static | The profile: call sites must stay monomorphic; inlining has depth and size budgets (`MaxInlineLevel`, `FreqInlineSize`) |
| What breaks it | `dyn`, debug builds, very large functions | Megamorphic lambda sites (profile pollution, Chapter 7.2), deep pipelines past the inline budget, deoptimization |
| Pipeline objects | Stack values, never allocated | Heap objects, eliminated only when escape analysis succeeds |
| Boxed elements | Only if you write `Box` | `Stream<Long>` boxes unless you use `LongStream` |
| First request after deploy | Already optimized | Interpreted or C1-compiled until warm |

> **Analogy limit.** "The JIT does the same thing at run time" is fair for a single hot, monomorphic pipeline in a
> long-running process. It stops being true for *which* pipelines get optimized. A Rust chain is optimized because of
> its types, identically on every run and from the first call. A Java pipeline is optimized because of its history,
> and a shared utility method called with many lambdas can lose its optimizations when a new caller shows up.

### 9. Production scenario

**Meridian's settlement batch job** (the Rust job from Chapter 8.1) reconciles processor settlement files against the
ledger every night, about 40 million rows across CSV and ISO 20022 XML formats from different processors. Its
iterator guidelines were written from this chapter's measurements, and one of them is about *not* optimizing:

- **Formats are chosen per file, so `dyn` lives there.** Each parser is a `Box<dyn Iterator<Item = Result<Row, ParseError>>>`
  chosen by file type. At 1.63 ns per `next()`, 40M rows cost about 65 ms of dispatch per night. It isn't worth one
  line of generic code. The team wrote the arithmetic down so nobody would "optimize" it later.
- **Per-row work is static.** Normalization and matching are generic functions over `impl Iterator<Item = Row>`, so
  they inline into one loop per format.
- **No mid-pipeline collects.** The review checklist flags a `collect()` whose result is iterated exactly once. The
  aggregation is one `try_fold` into a per-merchant accumulator (a `BTreeMap`, so the output is ordered and stable).
- **The only per-row allocation that mattered was a `String` per field** in the CSV parser, not the iterators. It was
  replaced by borrowed `&str` fields from a reused line buffer (a lending reader, Chapter 10.2), which is where the
  job's time actually went.

The lesson the team drew was proportion. The zero-cost rules matter in inner loops over hundreds of millions of
elements, or in per-request code on the latency path. Elsewhere they're noise, and allocation is almost always the
bigger fish.

### 10. Failure scenario

**The blocklist that kept its candidates.** Meridian's fraud service refreshes a merchant-and-card blocklist every 10
minutes. The refresh loads about 8 million candidate entries from the risk database into a `Vec<Entry>` (32 bytes
each), keeps the ones currently active (about 2%), and swaps the result into place for the scoring threads:

```rust,ignore
// Illustrative (Meridian's code, not a listing); the mechanism is measured in listing ch03-06.
let active: Vec<Entry> = candidates.into_iter().filter(|e| e.is_active(now)).collect();
blocklist.store(Arc::new(active)); // readers use it until the next refresh
```

The expected memory was 160,000 entries × 32 bytes ≈ 5 MB. The pods' memory showed about **256 MB per blocklist**,
held for 10 minutes. During each refresh, the old and new lists coexisted with the new candidate buffer, so peaks
reached the 1 GB container limit and pods were OOM-killed a few times a day.

It's the in-place collect row from §5, at scale. The `filter` ran in place, so `active` inherited the candidates'
buffer: capacity 8 million, length 160,000. Nothing leaked, and every byte was freed when the list was replaced. It
was just 50× bigger than anyone thought, for its whole lifetime.

The fix was one line, `active.shrink_to_fit()` before publishing. The listing's last row shows it costs one allocation
and a copy of the kept elements. The team also added a gauge for `capacity × size_of::<Entry>()` of the published list,
because `len()` was the number their dashboards had been showing.

The general lesson: **`len` is not memory.** Any long-lived `Vec` built by transformation (in-place collect, `retain`,
`truncate`, `clear`, or `drain`) keeps its capacity. Chapter 9.1 made the same point for `clear()` in the ingestion
batch buffer. The iterator version is harder to spot because nothing in the code mentions the old vector.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part X).*

1. What exactly is the type of `v.iter().filter(p).map(f)`? Where does it live, and how big is it?
2. Name the four ingredients that let an iterator chain compile to the same code as a loop. What happens when each is
   missing?
3. What is internal iteration? Why can `a.iter().chain(b.iter()).sum()` be faster than a `for` loop over the same
   chain?
4. Why does `sum()` on a `Box<dyn Iterator>` call `next()` through the vtable for every element, even though `sum` is
   an iterator method?
5. When does `collect()` reuse the source `Vec`'s buffer? What can go wrong with that, and how do you fix it?
6. Does indexing in a loop always keep bounds checks? What did LLVM do in `add_indexed`, and what's the semantic
   difference between the three versions?
7. Why are debug-build timings of iterator code meaningless for release decisions?
8. Compare how Rust and the HotSpot JIT eliminate the abstraction cost of a stream pipeline. When does each fail?
9. You're reviewing a PR that replaces an iterator chain with index loops "for performance." What do you ask for?

### 12. Exercises

- **Beginner.** Predict the `size_of_val` of `v.iter().zip(w.iter()).skip(3).step_by(2)` for `v, w: Vec<u32>`. Check
  it with `type_name_of_val` and `size_of_val`.
- **Intermediate.** Implement `fold` for the hand-rolled `MyMap` and `MyFilter` (listing `ch03-02`), so that a chain of
  them uses internal iteration. Verify that `sum()` over your chain calls your `fold` (use a `Cell` counter).
- **Advanced.** Write a `Chain`-like adapter `MyChain<A, B>` with only `next()`. Emit its release assembly for a sum,
  then add a `fold` override and compare. Do you reproduce the two-loop structure?
- **Systems.** Extend listing `ch03-06` with `u8 -> u8`, `[u8; 3] -> u8`, and `u64 -> (u32, u32)`. Predict each row
  before running. Then find the documentation of `Vec`'s `FromIterator` impl and compare what it promises with what you
  measured.
- **Architecture.** Write a one-page iterator guideline for a team's hot paths: which shapes are allowed, where `dyn`
  may appear, when `collect()` needs a comment, and which measurement is required to overrule the guideline.

### 13. Debugging exercise

A developer reports: "The iterator version of `sum_even_squares` is 25× slower than the loop version on my machine,
so I'm converting our hot paths to index loops." Their numbers, from `cargo test` with a timing assertion: iterator
10.24 ns per element, index loop 15.28 ns per element.

1. What's wrong with the measurement? (Hint: compare with this chapter's table.)
2. Their own numbers contradict their claim. How?
3. What should the measurement setup be, and what result do you predict?
4. Name one real case in this chapter where an iterator *is* much slower than a loop, and the fix that isn't "use a
   loop."

### 14. Design exercise

**A plugin pipeline for Meridian's fraud features.** The fraud team wants data scientists to add feature extractors
without touching the core crate. Each extractor consumes a merchant's recent transactions (up to ~5,000) and produces
one `f64`. There are about 400 features per score, and the service does 50K scores/s (Chapter 1.2).

Design the extractor interface:

- `fn extract(&self, txns: &mut dyn Iterator<Item = &Txn>) -> f64`, `fn extract(&self, txns: &[Txn]) -> f64`, or a
  generic method (and what that does to dyn-compatibility, Chapter 6.4).
- Where the dynamic dispatch happens (per feature, per transaction, or per batch of features), with the arithmetic:
  400 features × 5,000 transactions × 50K scores/s, at the per-call costs measured in this chapter.
- How the core can compute shared intermediate values once (for example, sorted amounts) and give them to every
  extractor.
