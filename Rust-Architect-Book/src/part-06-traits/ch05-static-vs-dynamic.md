# Chapter 6.5 — Static vs Dynamic Dispatch: The Architect's Decision

> **Where this sits:** Part VI · Traits · chapter 5 of 5
> **Prerequisites:** Chapters 6.1–6.4. Chapter 2.5 (the expression problem), Chapter 2.4 (function items vs pointers).
> **After this chapter you can:** choose among generics, enums, and trait objects on seven explicit dimensions
> (compile time, binary size, dispatch cost, cache locality, extensibility, ABI, plugins) with a decision matrix; quote
> measured numbers for the dispatch difference and explain each one from the generated assembly; apply the ratio test
> that tells you when dispatch cost matters at all; compose static cores with dynamic edges; and explain why a Rust
> plugin boundary needs a C ABI (verified under Miri) rather than `dyn Trait`.

---

## Pass 1 · User level — *Three ways to say "one of several types"*

### 1. Problem

Meridian's fraud-scoring library (Chapter 1.2: 50K scores/s, p99 under 5 ms, a Rust library called from Java through
FFM) runs a set of rules against every transaction. Two kinds of rule exist:

- A **core set** owned by the fraud team: amount thresholds, velocity, country risk. They change with releases, run on
  every score, and some of them loop over thousands of historical transactions per score.
- **Experimental rules** from the risk-analytics team, switched on per market by configuration and changed weekly.
  Analytics wants them loadable without a fraud-library release.

Each part of that needs a different answer to "how does code call a rule?" Chapters 6.1–6.4 gave three mechanisms.
This chapter is about choosing, with measurements instead of folklore.

### 2. Mental model

```text
                      GENERICS  T: Rule                ENUM  Rule::{A, B, C}             TRAIT OBJECT  dyn Rule
 the set of types     open (any T), but ONE per use    CLOSED, listed in one place       OPEN, mixed freely at run time
 decided              at compile time, per call site   at compile time, by the enum      at run time, by the value
 call mechanism       direct call, usually inlined     branch on the tag, arms inlined   indirect call through a vtable
 code                 one copy per T                   one copy                          one copy + one vtable per type
 adding a TYPE        free (anyone)                    edit the enum + every match       free (anyone, even via config)
 adding an OPERATION  edit the trait + every impl      one new function with a match     edit the trait + every impl
```

The last two rows are Chapter 2.5's **expression problem**, now with all its Rust forms visible. Enums make operations
cheap to add and variants expensive. Traits make types cheap to add and operations expensive. Generics and trait objects
sit on the same side, since both are traits. They differ in *when* the type is chosen, and that's what drives cost.

A fourth, often forgotten option is the **function pointer** (`fn(&Txn) -> u32`). It's dynamic dispatch without a vtable:
one indirect `call`, no captured state (Chapter 2.4). The factory registry in Chapter 6.4 used exactly that. And
closures come in both flavors: `F: Fn(&Txn) -> u32` is static, `Box<dyn Fn(&Txn) -> u32>` is dynamic.

### 3. Rust code

**`impl Trait` in return position is still one type.** The most common first collision with this decision (listing
`ch05-03-impl-trait-branches.rs`):

```rust,compile_fail
trait Fee {
    fn fee(&self, amount: i64) -> i64;
}

struct Percent {
    bps: i64,
}
struct Flat {
    cents: i64,
}

impl Fee for Percent {
    fn fee(&self, amount: i64) -> i64 {
        amount * self.bps / 10_000
    }
}
impl Fee for Flat {
    fn fee(&self, _amount: i64) -> i64 {
        self.cents
    }
}

/// "Return some Fee": but `impl Trait` means ONE concrete type chosen by the function.
fn fee_for(country: &str) -> impl Fee {
    if country == "IE" { Percent { bps: 29 } } else { Flat { cents: 30 } }
}

fn main() {
    println!("{}", fee_for("IE").fee(10_000));
}
```

```text
error[E0308]: `if` and `else` have incompatible types
26 |     if country == "IE" { Percent { bps: 29 } } else { Flat { cents: 30 } }
   |                          -------------------          ^^^^^^^^^^^^^^^^^^ expected `Percent`, found `Flat`
   |                          |
   |                          expected because of this
help: you could change the return type to be a boxed trait object
25 + fn fee_for(country: &str) -> Box<dyn Fee> {
```

[LANG] Return-position `impl Fee` hides the type from the caller, but it's still *one* static type, chosen by the
function body. rustc suggests `Box<dyn Fee>`, and that's one of two correct answers (listing
`ch05-04-return-choices.rs`, verified):

```rust,ignore
/// Option 1: an open set. Any Fee type, one heap allocation, dynamic dispatch.
fn fee_boxed(country: &str) -> Box<dyn Fee> {
    if country == "IE" { Box::new(Percent { bps: 29 }) } else { Box::new(Flat { cents: 30 }) }
}

/// Option 2: a closed set. No allocation; a match instead of a vtable.
enum AnyFee {
    Percent(Percent),
    Flat(Flat),
}

impl Fee for AnyFee {
    fn fee(&self, amount: i64) -> i64 {
        match self {
            AnyFee::Percent(p) => p.fee(amount),
            AnyFee::Flat(f) => f.fee(amount),
        }
    }
}

fn fee_enum(country: &str) -> impl Fee {
    if country == "IE" { AnyFee::Percent(Percent { bps: 29 }) } else { AnyFee::Flat(Flat { cents: 30 }) }
}
```

```text
IE: boxed 29 / enum 29
DE: boxed 30 / enum 30
```

Same behavior, different architecture. `fee_boxed` lets any crate add a fee type. `fee_enum` doesn't, and needs no
allocation. Implementing `Fee` for the enum by delegating to its variants keeps callers generic. That's the idiomatic
bridge between the two worlds.

**Composition: static, dynamic, and hybrid.** A middleware stack can be one static type, a list of trait objects, or a
static core with a dynamic slot (listing `ch05-02-static-stack.rs`, verified):

```rust
use std::any::type_name_of_val;
use std::mem::size_of_val;

#[derive(Default)]
struct Request {
    trace: Vec<&'static str>,
}

trait Middleware {
    fn handle(&self, req: &mut Request) -> Result<(), String>;
}

struct Auth;
struct RateLimit {
    per_sec: u32,
}
struct TenantTag;
struct GeoFence;

impl Middleware for Auth {
    fn handle(&self, req: &mut Request) -> Result<(), String> {
        req.trace.push("auth");
        Ok(())
    }
}
impl Middleware for RateLimit {
    fn handle(&self, req: &mut Request) -> Result<(), String> {
        req.trace.push(if self.per_sec > 0 { "ratelimit" } else { "ratelimit(off)" });
        Ok(())
    }
}
impl Middleware for TenantTag {
    fn handle(&self, req: &mut Request) -> Result<(), String> {
        req.trace.push("tenant");
        Ok(())
    }
}
impl Middleware for GeoFence {
    fn handle(&self, req: &mut Request) -> Result<(), String> {
        req.trace.push("geofence");
        Ok(())
    }
}

/// Static composition: the whole pipeline is ONE type, so every call is a direct (inlinable) call.
struct Stack<A, B> {
    first: A,
    rest: B,
}

impl<A: Middleware, B: Middleware> Middleware for Stack<A, B> {
    fn handle(&self, req: &mut Request) -> Result<(), String> {
        self.first.handle(req)?;
        self.rest.handle(req)
    }
}

/// The dynamic extension point: a list of plugins chosen at run time.
struct Plugins(Vec<Box<dyn Middleware>>);

impl Middleware for Plugins {
    fn handle(&self, req: &mut Request) -> Result<(), String> {
        for p in &self.0 {
            p.handle(req)?;
        }
        Ok(())
    }
}

fn run(name: &str, m: &dyn Middleware, size: usize, ty: &str) {
    let mut req = Request::default();
    m.handle(&mut req).unwrap();
    println!("{name:<8} {size:>3} bytes  {:?}", req.trace);
    println!("         type = {}", ty.replace("playground::", ""));
}

fn main() {
    let fixed = Stack { first: Auth, rest: Stack { first: RateLimit { per_sec: 500 }, rest: TenantTag } };

    let dynamic = Plugins(vec![Box::new(Auth), Box::new(RateLimit { per_sec: 500 }), Box::new(TenantTag)]);

    let hybrid = Stack {
        first: Auth,
        rest: Stack { first: RateLimit { per_sec: 500 }, rest: Plugins(vec![Box::new(TenantTag), Box::new(GeoFence)]) },
    };

    run("static", &fixed, size_of_val(&fixed), type_name_of_val(&fixed));
    run("dynamic", &dynamic, size_of_val(&dynamic), type_name_of_val(&dynamic));
    run("hybrid", &hybrid, size_of_val(&hybrid), type_name_of_val(&hybrid));
}
```

```text
static     4 bytes  ["auth", "ratelimit", "tenant"]
         type = Stack<Auth, Stack<RateLimit, TenantTag>>
dynamic   24 bytes  ["auth", "ratelimit", "tenant"]
         type = Plugins
hybrid    32 bytes  ["auth", "ratelimit", "tenant", "geofence"]
         type = Stack<Auth, Stack<RateLimit, Plugins>>
```

The static pipeline is **4 bytes** (the `u32` in `RateLimit`, since `Auth` and `TenantTag` are zero-sized), and its
*type is the pipeline*: `Stack<Auth, Stack<RateLimit, TenantTag>>`. Changing the order changes the type, so it can't
come from a config file. The dynamic one is a 24-byte `Vec` header plus heap objects, and its type says nothing about
its contents. The hybrid is what production systems usually want: the parts that never change are static, and one
`Plugins` slot at the end is open. This is the shape of Tower's `Layer`/`Service` stacks and `BoxService`
(Chapter 22.3), where a type-erased box is used at the boundary to stop type names like the one above from growing without
limit.

---

## Pass 2 · Systems level — *Measured, then explained*

### 4. Under the hood

**The benchmark.** Listing `ch05-01-dispatch-bench.rs` computes a fee for 1,000,000 fee components of three kinds
(`Percent`, `Flat`, `Tiered`), mixed pseudo-randomly so there's no pattern to learn, in four representations:

- `Vec<Box<dyn Fee>>` in mixed order
- the same, **grouped by type** (all `Percent`s, then `Flat`s, then `Tiered`s)
- `Vec<FeeKind>` (an enum)
- **one `Vec` per concrete type**, summed with a generic `sum_static<F: Fee>`

It counts heap allocations with Part III's counting allocator and times the best of 5 passes with `Instant`. The
core (excerpt; the allocator and table printing are omitted):

```rust,ignore
fn sum_dyn(fees: &[Box<dyn Fee>], amount: i64) -> i64 {
    fees.iter().map(|f| f.fee(amount)).sum()
}
fn sum_enum(fees: &[FeeKind], amount: i64) -> i64 {
    fees.iter().map(|f| f.fee(amount)).sum()
}
fn sum_static<F: Fee>(fees: &[F], amount: i64) -> i64 {
    fees.iter().map(|f| f.fee(amount)).sum()
}
    // ...
    let (s1, t_dyn) = best_ns(5, || sum_dyn(black_box(&dyn_mixed), amount));
    let (s2, t_grp) = best_ns(5, || sum_dyn(black_box(&dyn_grouped), amount));
    let (s3, t_enum) = best_ns(5, || sum_enum(black_box(&enums), amount));
    let (s4, t_static) = best_ns(5, || {
        sum_static(black_box(&pct), amount) + sum_static(black_box(&flat), amount) + sum_static(black_box(&tier), amount)
    });
    assert!(s1 == s2 && s2 == s3 && s3 == s4);
```

Release build on the Rust Playground, rustc 1.98.1. **Four separate runs, each one run on a shared machine, so noisy**,
but they agreed to within 0.02 ns. The first run printed:

```text
1000000 fees, total = 32083167
design                     allocations      slot bytes    ns/fee
Vec<Box<dyn Fee>> mixed        1000001              16      3.28
Vec<Box<dyn Fee>> grouped      1000001              16      2.00
Vec<FeeKind> (enum)                  1              32      1.02
one Vec per type (static)            3      8 / 8 / 24      0.81
```

| Design | Allocations to build | ns per fee (4 runs) |
|---|---|---|
| `Vec<Box<dyn Fee>>`, mixed order | 1,000,001 | 3.26–3.28 |
| `Vec<Box<dyn Fee>>`, grouped by type | 1,000,001 | 1.99–2.00 |
| `Vec<FeeKind>` (enum) | 1 | 1.01–1.02 |
| One `Vec` per type, generic sum | 3 | 0.80–0.82 |

About 4× between the extremes, for the *same arithmetic*. The numbers are specific to this machine and workload. What
generalizes is the mechanism, so here it is from the assembly (listing `ch05-05-enum-codegen.rs`, the same two loops in
a library, release build, comments added):

```text
playground::sum_dyn:                          ; one indirect call per element
.LBB0_3:
	mov	rdi, qword ptr [r15 + r13]            ; data pointer
	mov	rax, qword ptr [r15 + r13 + 8]        ; vtable pointer
	mov	rsi, rbx                              ; amount
	call	qword ptr [rax + 24]              ; Fee::fee through the vtable
	add	r12, rax                              ; running total lives in r12 (callee-saved)
	add	r13, 16
	cmp	r14, r13
	jne	.LBB0_3

playground::sum_enum:                         ; the match, inlined into the loop
.LBB1_4:
	mov	rax, qword ptr [rdi]                  ; discriminant
	mov	rdx, qword ptr [rdi + 8]              ; first field of the variant
	test	rax, rax
	je	.LBB1_9                               ; Percent → multiply, then /10_000 below
	cmp	eax, 1
	je	.LBB1_8                               ; Flat → just add `cents`
	xor	eax, eax                              ; Tiered, BRANCH-FREE:
	cmp	rcx, rdx                              ;   amount >= threshold ?
	setge	al                                ;   0 or 1
	mov	rax, qword ptr [rdi + 8*rax + 16]     ;   load low_bps or high_bps by index
	imul	rax, rcx
	jmp	.LBB1_7
	...
.LBB1_7:                                      ; x / 10_000 without a divide:
	imul	r9                                ;   multiply by a magic constant...
	mov	rax, rdx
	shr	rax, 63
	sar	rdx, 11                               ;   ...and shift
	add	rdx, rax
```

Reading the four rows against this:

1. **Mixed `dyn` (3.3 ns)**: a call and a return per element, argument setup, and an **indirect branch whose target
   changes unpredictably**. Every function body is out of the optimizer's reach from the loop.
2. **Grouped `dyn` (2.0 ns)**: the *same instructions*, and the boxes were also allocated in order in both cases, so
   memory access is similar. What changed is that the call target now repeats for long runs, so the indirect-branch
   predictor is almost always right. The 1.3 ns gap is therefore **mostly branch misprediction**. That's predicted from
   mechanism: confirm it with `perf stat -e branch-misses` on Linux (exercise; `perf` isn't available on the Playground).
3. **Enum (1.0 ns)**: two ordinary conditional branches (still unpredictable in mixed order), but no call. The `Tiered`
   arm became branch-free (`setge` plus an indexed load). The optimizer sees all three bodies and schedules them
   together.
4. **Static per type (0.8 ns)**: each loop handles one type, with no dispatch at all, the fee arithmetic inlined, and
   `Flat`'s loop reduced to summing a field. The remaining gap to the enum is, predicted from mechanism, the
   discriminant test and the wider 32-byte elements.

Grouping trick: "sort by type" turns dynamic dispatch into predictable dispatch, and one step further, splitting by
type turns it into static dispatch. That's the data-oriented design (entity-component-system) idea, and it's available
whenever the *order* of processing doesn't matter.

**Where monomorphization shows up.** Chapter 6.4's assembly already showed the static side's price:
`total_area_static::<Circle>` and `total_area_static::<Square>` are two functions, and `total_area_dyn` is one. Part VII,
Chapter 7.1 follows a generic from call site to binary. Chapter 7.3 measures what those copies cost in compile time and
code size, and how to cap them. For this chapter, the rule is enough: **static dispatch multiplies code by the number of
types used, and dynamic dispatch multiplies calls by an indirection.**

### 5. Memory

From the same run:

```text
 dyn Fee:   Vec of 16-byte fat pointers  ─►  1,000,000 separate heap objects (8–24 bytes each, plus allocator overhead)
 FeeKind:   Vec of 32-byte enums (every element sized for the largest variant, Tiered = 24 + tag)
 static:    three Vecs of 8-, 8-, and 24-byte elements, exactly sized, fully contiguous
```

- **Allocation count** is the headline: 1,000,001 vs 1 vs 3, measured. Building, and later freeing, a million boxes is
  real time and real allocator contention in multi-threaded code.
- **Per-object overhead** [LIB]: glibc's malloc can't hand out a chunk smaller than its minimum (32 bytes on 64-bit
  glibc, even for an 8-byte request), so a boxed 8-byte `Flat` costs at least a 32-byte chunk plus its 16-byte fat
  pointer. That's 48 bytes, against 32 in the enum `Vec` and 8 in its own `Vec`. Part IX (allocation behavior) and
  Part XX (allocation rate) return to allocators.
- **The enum's weakness** is size: every element pays for the largest variant. One rare 200-byte variant makes every
  element 200+ bytes. The fix is boxing the rare large variant (Chapter 2.6's market-data queue did exactly that).

### 6. CPU / OS

**The ratio test.** Dispatch overhead matters in proportion to *calls per unit of useful work*:

```text
 overhead fraction ≈ (extra ns per dynamic call) / (ns of work per call)
```

Order-of-magnitude, using this chapter's measurements as the "extra ns" (about 1–2.5 ns per call against the enum or
static forms; your workload's numbers will differ):

| Call site | Work per call | Dispatch share | Verdict |
|---|---|---|---|
| Gateway middleware: ~10 dynamic calls per request, ~0.5 ms CPU per request (Chapter 1.2) | ~50 µs | ~0.005% | Irrelevant: use `dyn` freely |
| Fraud rules: ~100 rule calls per score, ~3 ms CPU per score | ~30 µs | < 0.01% | Irrelevant at the rule level |
| A fraud *feature* looping over 5,000 past transactions, calling a `dyn` accessor per element | a few ns | **50–300%** | Matters: go static or enum *inside the loop* |
| A byte-level decoder calling `dyn` per field or per byte | ~1 ns | **> 100%** | Matters, and the lost vectorization costs more than the calls |

The architect's version of the static-vs-dynamic question isn't "which is faster?" (static, when it matters) but
"**where is the loop?**" Put dynamic dispatch *outside* the hot loops and static dispatch *inside* them, and both costs
vanish.

**The OS doesn't see dispatch**, with one exception from Chapter 6.4: kernels mitigate Spectre v2 on indirect branches,
so indirect calls cost more in kernel code than in user code.

---

## Pass 3 · Architect level — *The decision matrix*

### 7. Trade-offs

The Part's required comparison, on seven dimensions. There is **no universal winner**: each column wins somewhere.

| Dimension | Generics (`T: Trait`) | Enum (closed set) | `dyn Trait` |
|---|---|---|---|
| **Compile-time cost** | Highest. Each instantiation is compiled and optimized separately, in the *user's* crate. Deeply nested generic types multiply it (Chapter 7.3) | Low: one copy | Lowest: one copy, compiled once in the defining crate |
| **Binary size** | One copy per type used, which can bloat with many types or large bodies | One copy; `match` arms inline | One copy, plus one small vtable per (type, trait) |
| **Runtime dispatch** | None: direct calls, inlining, vectorization. 0.8 ns/fee measured | Tag branch, arms inlinable. 1.0 ns/fee | Indirect call, no inlining across it. 2.0–3.3 ns/fee |
| **Cache locality** | `Vec<T>` contiguous, exactly sized; one type per collection | Contiguous; each element sized for the largest variant | Pointer per element, objects scattered, 1 allocation each (measured: 1,000,001) |
| **Extensibility** | New types by anyone, but each collection or call site is fixed to one type at compile time | **Closed**: new variants mean editing the enum and every `match` (the compiler finds them). New operations are cheap | **Open**: new types by anyone, even chosen by config at run time. New operations mean editing the trait |
| **ABI considerations** | Can't cross a binary boundary: generics are compiled into the caller | Layout unspecified unless `#[repr(C)]`/`#[repr(u8)]` (Part V) | Vtable layout unspecified: **not a plugin ABI** (below) |
| **Plugin architectures** | Compile-time plugins only: features, generic composition (`Stack<A, B>`) | Plugins are new variants, so recompile the host | In-process runtime plugins built *together with the host*; registries and factories |

**Rules of thumb** that fall out of the matrix:

1. **Inside hot loops**: generics, or an enum if the set is closed and mixed. Never a per-element `Box<dyn>`.
2. **For closed sets you own** (states, message kinds, fee types): an enum. The compiler checks exhaustiveness, and
   there are no allocations.
3. **For open sets, configuration-driven composition, and heterogeneous collections of long-lived objects**: `dyn`.
4. **For libraries**: accept generics (`impl Trait` parameters) so callers keep static dispatch. Internally, erase
   with `dyn` where a generic would spread through every signature or bloat compile times.
5. **Hybrid by default in services**: a static core with dynamic edges, like the `Stack<…, Plugins>` shape above.
6. **To cut compile time**: turn a deep generic stack into `Box<dyn …>` at module boundaries. That's a legitimate trade
   of nanoseconds for minutes.

**ABI and plugins: why `dyn` isn't a plugin boundary.** [LANG] [RUSTC] Rust has no stable ABI. Struct layouts (default
repr), enum layouts, the `extern "Rust"` calling convention, and vtable layout may all change between compiler
versions, and even between builds with different flags. A plugin compiled separately that hands the host a
`Box<dyn Rule>` only works when both sides were built by the same compiler with compatible settings, and nothing checks
that. Real plugin boundaries use one of three designs:

| Boundary | Isolation | Cost per call | Notes |
|---|---|---|---|
| **C ABI** (`extern "C"` + `#[repr(C)]`), `cdylib` loaded at run time | None: same process, a crash takes the host down | An indirect call | Stable across compilers. You write the vtable yourself (below). Loading and symbol lookup are Part XVI |
| **WebAssembly** components in an embedded runtime | Sandboxed memory | A boundary crossing (roughly tens of ns, order of magnitude), plus data copying | Language-neutral; a runtime such as `wasmtime` (not available on the Playground) |
| **Out of process** (gRPC/HTTP) | Full process isolation | A network round trip (µs to ms) | Independent deploys and languages; the default for analytics-owned code |

What "write the vtable yourself" means (listing `ch05-06-c-abi-vtable.rs`, verified, and clean under **Miri**):

```rust
use std::ffi::c_void;

/// A hand-built "trait object" with a C-compatible layout. A plugin boundary needs this,
/// because Rust's own vtable layout (and `dyn Trait` in general) is not a stable ABI.
#[repr(C)]
pub struct RuleVTable {
    pub score: extern "C" fn(data: *const c_void, amount_cents: i64) -> u32,
    pub drop: unsafe extern "C" fn(data: *mut c_void),
}

#[repr(C)]
pub struct FfiRule {
    data: *mut c_void,
    vtable: &'static RuleVTable,
}

impl FfiRule {
    pub fn score(&self, amount_cents: i64) -> u32 {
        (self.vtable.score)(self.data, amount_cents)
    }
}

impl Drop for FfiRule {
    fn drop(&mut self) {
        // SAFETY: `data` was produced by the constructor paired with this vtable (Box::into_raw),
        // and FfiRule is not Clone, so this is the only drop of that allocation.
        unsafe { (self.vtable.drop)(self.data) }
    }
}

// --- the "plugin side": an ordinary Rust type exported through the C-ABI table ---
struct LargeAmount {
    over_cents: i64,
}

extern "C" fn large_amount_score(data: *const c_void, amount_cents: i64) -> u32 {
    // SAFETY: the host only passes back the `data` pointer created by `new_large_amount`,
    // which points to a live LargeAmount until the drop entry is called.
    let rule = unsafe { &*(data as *const LargeAmount) };
    if amount_cents > rule.over_cents { 60 } else { 0 }
}

unsafe extern "C" fn large_amount_drop(data: *mut c_void) {
    // SAFETY: `data` came from Box::into_raw in `new_large_amount`; the caller guarantees a single call.
    drop(unsafe { Box::from_raw(data as *mut LargeAmount) });
}

static LARGE_AMOUNT_VTABLE: RuleVTable = RuleVTable { score: large_amount_score, drop: large_amount_drop };

/// What a plugin would export from a cdylib (loading it is Part XVI's job).
pub extern "C" fn new_large_amount(over_cents: i64) -> FfiRule {
    let data = Box::into_raw(Box::new(LargeAmount { over_cents })) as *mut c_void;
    FfiRule { data, vtable: &LARGE_AMOUNT_VTABLE }
}

fn main() {
    let rule = new_large_amount(500_000);
    println!("score(1_200) = {}, score(750_000) = {}", rule.score(1_200), rule.score(750_000));
    println!("size_of::<FfiRule>() = {} bytes", std::mem::size_of::<FfiRule>());
} // `rule` is dropped here: the plugin's own drop entry frees its Box
```

```text
score(1_200) = 0, score(750_000) = 60
size_of::<FfiRule>() = 16 bytes
```

It's the same shape as Chapter 6.4's fat pointer, 16 bytes of data pointer plus vtable pointer, but every part of it is
now **specified**: `#[repr(C)]` field order, the `extern "C"` calling convention, and a drop entry the *plugin* provides,
so memory is freed by the allocator that created it. Everything `dyn` did automatically is now manual and `unsafe`, and
the raw pointer makes `FfiRule` neither `Send` nor `Sync` until you add a documented `unsafe impl`. That's the price of
an ABI you control. (Not verified here: building the plugin as a `cdylib` and loading it with a library such as
`libloading`. That needs two separately compiled artifacts, which is Part XVI's territory.)

### 8. Java comparison

**Java chooses dynamic, and the JIT claws back static.** Every Java instance-method call that isn't to a private or
final method is virtual. At
run time, HotSpot profiles each call site: one receiver class seen (*monomorphic*) gets a guarded direct call that C2
can inline, two (*bimorphic*) get two guards, more (*megamorphic*) fall back to a table dispatch. If a new class shows
up later, the JIT *deoptimizes* and recompiles. The result is often excellent, and it's speculative: performance
depends on the profile, warm-up, and the classes loaded so far. Sort order changed, and a site that was monomorphic
becomes megamorphic (the Java equivalent of this chapter's mixed-vs-grouped row).

**Rust chooses at compile time, and never speculates.** Generics and enums are static because the source says so. `dyn`
is dynamic because the source says so, and stays dynamic unless LLVM can *prove* the type, or PGO promotes a hot target
(Part XX). There's no warm-up and no deoptimization, and no surprise in either direction: the assembly you read in this
chapter is what runs.

**Erasure vs monomorphization** (brief, since Part VII, Chapter 7.2 goes deep). Java compiles `List<Integer>` once, for
`Object`, so generics are always "dynamic" underneath, and values are boxed. Rust compiles `Vec<i64>` specifically, so
the static row of the matrix is available in the first place. The measured 0.8 ns path, contiguous `i64` fields and
inlined arithmetic, is something a `List<Fee>` of objects can't express.

**Sealed interfaces vs enums.** Java 17's `sealed interface Fee permits Percent, Flat, Tiered` plus Java 21's pattern
`switch` gives Java an exhaustiveness-checked closed set: the enum column, in Java. The difference is representation.
Java's variants are still separate heap objects behind references (the dyn column's memory layout), while a Rust enum
stores them inline (the enum column's). **ServiceLoader** is Java's `dyn` plugin mechanism, and it works across
separately compiled JARs because the JVM *has* a stable binary interface (bytecode and class files). That's the
capability Rust lacks natively, and the reason for §7's C ABI.

> **Analogy limit.** "Rust generics are like C++ templates, and `dyn` is like Java interfaces" is a good first map, but
> it misleads twice. Rust generics are type-checked at the definition, unlike templates (Chapter 6.1). And a Rust `dyn`
> call is *more* predictable than a Java interface call, not less: its cost is always one indirect call, never a
> deoptimization storm, and never an inlined fast path either. Java can beat a Rust `dyn` call site by inlining a
> monomorphic target that Rust must call indirectly. Rust beats Java by making that site static in the source.

### 9. Production scenario

**Meridian's fraud-rule engine, redesigned with the matrix.**

- **Feature computation (the hot loops)**: generic functions over concrete, contiguous types
  (`fn velocity<S: TxnSource>(src: &S, window: Duration)`), with past transactions stored as plain structs in `Vec`s.
  No `dyn` inside any loop over transactions. That's where the ratio test says dispatch costs 50%+.
- **Core rules (closed, owned by the fraud team)**: an enum `CoreRule { LargeAmount{..}, Velocity{..},
  RiskyCountry{..} }` with `impl Rule for CoreRule`. Adding a core rule is a code change the compiler walks through
  every `match`. That's exactly what the fraud team wants.
- **Experimental rules (open, owned by analytics)**: `Vec<Box<dyn Rule + Send + Sync>>` built from config through a
  factory registry, as in Chapter 6.4. That's in-process, so these rules ship *with* a fraud-library release, since
  `dyn` isn't a plugin ABI.
- **Truly independent analytics logic**: out of process. Analytics runs a scoring sidecar, and the fraud library calls
  it only for markets that opt in, under a strict timeout. The team rejected a C-ABI `cdylib` plugin: it would give
  analytics code the power to crash the fraud library's process, and the weekly-change requirement didn't justify that
  risk.

The engine type is the hybrid shape from §3: static core, one dynamic slot. Measured against the ratio test, the
dynamic slot's dispatch cost is invisible at the rule level (~30 µs of work per call), and the static core keeps the
per-transaction loops tight.

### 10. Failure scenario

**`Box<dyn Field>` per field.** A market-data team wrote a message decoder around a trait, `trait Field { fn
decode(&mut self, buf: &[u8]) -> usize; }`, and represented each message as `Vec<Box<dyn Field>>`, built fresh per
message. It was elegant, and new field types could be added anywhere. In production, a feed with about 30 fields per
message at 2M messages/s did 60M boxed allocations and 60M unpredictable indirect calls per second, on a code path whose
real work per field is a few nanoseconds of byte shuffling. The ratio test's worst row, in production.

This chapter's benchmark is the same structure in miniature: a million boxed, mixed-type objects cost 1,000,001
allocations and about 3.3 ns each to call, against 0.8 ns and 3 allocations for the static layout. The per-field work
here is similar in size (an arithmetic expression), which is exactly why dispatch dominated.

The redesign kept the trait and moved the loop:

- The **message schema** became an enum of field *kinds* (`U32Be`, `Price`, `Symbol`, …), a closed set owned by the
  protocol team, decoded by one `match` in a tight loop into a reused, flat struct.
- **Per-feed customization** stayed dynamic, at the right granularity: one `Box<dyn FeedHandler>` per *feed*, called
  once per *message*, not per field.
- A benchmark with the counting allocator went into CI, asserting **zero allocations per message** in steady state.

The lesson isn't "`dyn` is slow." It's that the unit of dynamic dispatch must match the unit of variation. Fields vary
by schema, which is known at compile time. Feeds vary by configuration, which isn't.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part VI).*

1. Compare generics, enums, and `dyn Trait` on extensibility using the expression problem. Which one lets a *third
   party* add a new case, and which lets you add a new *operation* cheaply?
2. Why does `fn f() -> impl Trait` fail when two branches return different types? Give the two fixes and what each
   costs.
3. The grouped `dyn` run was 1.3 ns per element faster than the mixed one with identical code. What changed, and how
   would you prove it?
4. Why was the enum faster than mixed `dyn` even though both have unpredictable branches?
5. State the ratio test. Apply it to a gateway middleware chain and to a per-byte decoder.
6. Why isn't `Box<dyn Trait>` a valid plugin ABI? What are the three real plugin boundaries, and their trade-offs?
7. When is replacing generics with `dyn` the *right* move? Name two reasons that aren't about run-time speed.
8. How does HotSpot's inline-cache strategy compare with Rust's static choice? Where can Java win, and where does Rust
   win?

### 12. Exercises

- **Beginner.** Rewrite `fee_for` to return `Box<dyn Fee>`, then the enum. Count allocations per call for each with the
  counting allocator.
- **Intermediate.** Add a 200-byte variant to `FeeKind` in the benchmark (for example, a tiered schedule with 25 tiers
  as an inline array). Predict and then measure the enum's ns/fee and memory. Then box the large variant and measure
  again.
- **Advanced.** Add a fifth design to the benchmark: `Vec<fn(i64) -> i64>` (function pointers, no captures). Where do
  you predict it lands, and why? Then try `Vec<Box<dyn Fn(i64) -> i64>>` with captured parameters.
- **Systems.** On Linux with `perf`, run the benchmark and record `branch-misses` and `instructions` for the mixed and
  grouped `dyn` runs separately. Does the miss count explain the 1.3 ns difference? (Budget: roughly 15–20 cycles per
  miss.)
- **Architecture.** Take a system you know and classify each polymorphic call site: closed or open set, hot or cold
  (ratio test), in-process or across a build boundary. Which ones are in the wrong column today?

### 13. Debugging exercise

A developer "simplified" the fee API from §3 by returning `impl Fee`, and got the E0308 shown there. Their next attempt
compiles:

```rust,ignore
fn fee_for(country: &str) -> Box<dyn Fee> {
    if country == "IE" { Box::new(Percent { bps: 29 }) } else { Box::new(Flat { cents: 30 }) }
}
```

It then went into the per-transaction pricing loop, called once per line item, for about 40M line items per hour.

1. What does this version cost per call compared with the enum version? Use this chapter's measurements and the
   counting-allocator result you'd expect.
2. The two fee types come from a country table that changes twice a year. Which column of the decision matrix does this
   belong in, and why?
3. Suppose the country table *did* have to be extensible by other teams without a release. Redesign so the dynamic
   decision is made once per *configuration load*, not once per line item.

### 14. Design exercise

**Meridian's pricing-rules platform.** Merchants get pricing rules (percent, flat, tiered, promotional, FX-adjusted).
Product teams add a new rule kind about once a quarter, and a partner program wants merchants' own rule logic
*uploaded* to run inside the pricing service. Pricing runs at about 20K quotes/s, each evaluating 5–50 rules.

Using the decision matrix, design:

- The representation of Meridian-owned rules (enum? generics? `dyn`?), and how a new quarterly rule kind is added.
- The boundary for partner-uploaded logic: C ABI, WebAssembly, or out of process? Consider isolation, per-call cost
  (apply the ratio test with 20K × 50 calls/s), deployment, and a malicious or buggy upload.
- What you measure before launch, and which measurement would make you change your mind.
