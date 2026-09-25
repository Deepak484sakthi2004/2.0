# Chapter 7.2 — Monomorphization vs Java Type Erasure

> **Where this sits:** Part VII · Generics and Monomorphization · chapter 2 of 3
> **Prerequisites:** Chapter 7.1 (instances, v0 symbols, per-type code). Chapter 6.4, *Trait Objects, vtables, and dyn
> Compatibility*, for the vtable details used in §4.
> **After this chapter you can:** explain, at bytecode and machine-code level, what Java's erasure does (casts, bridge
> methods, boxing, raw types, heap pollution) and what Rust does instead; measure the boxing cost; list what generic code
> can do with `T` in Rust but not in Java; place C#, Go, and C++ on the same map; explain how HotSpot's JIT recovers
> performance and where profile pollution stops it; and recognize "erasure-style Rust" as a design smell.

---

## Pass 1 · User level — *Two ways to compile one generic definition*

### 1. Problem

You have written Java generics for years, and your intuitions about them are *correct for Java*. `List<Long>` holds
references to `Long` objects. A generic method is one method. `T` doesn't exist at run time. Performance-sensitive code
avoids generics over numbers and uses `long[]` or a primitive-collections library. Every one of these intuitions is
wrong for Rust, in both directions. Rust generics cost less at run time than you expect (no boxing, no casts, and direct
calls), and more at build time (a copy per type). The expressive power differs too: Rust's generic code can do things
with `T` that Java's can't, and the reverse is also true.

This chapter puts the two implementations side by side, from the bytecode and assembly up to the architectural
consequences. You will need this whenever you port Java code, review a Rust design written by Java engineers, or
explain to a JVM team why the Rust service needs no warm-up and has a bigger binary.

### 2. Mental model

There are two classic ways to compile parametric polymorphism:

```text
 HOMOGENEOUS TRANSLATION (Java: erasure)          HETEROGENEOUS TRANSLATION (Rust: monomorphization)

 one body for all T                               one body per T actually used
 every T is represented the same way:             each T is represented as itself:
   a reference to a heap object                     i64 is 8 bytes inline, String is 24, MyType is its size
 operations on T = virtual/interface calls        operations on T = direct calls, usually inlined
 the compiler inserts casts where T flows out     no casts: the type is known in every copy
 primitives must be boxed to become a T           primitives are ordinary T's
 the JIT may specialize later, at run time,       all specialization happens at build time, in CI,
   guided by profiles, and may deoptimize           guided by the types, and is never undone
```

The rest of the landscape falls between these two poles:

| Language | Strategy | Primitives as `T` | `T` at run time | Specialization happens |
|---|---|---|---|---|
| **Java** | Erasure (homogeneous) | Boxed | Erased (declarations kept for reflection) | At run time, by the JIT, if profiles allow |
| **C#/.NET** | Reified; one shared body for reference types, one body per value type | Unboxed | Fully reified | At JIT (or AOT) time |
| **Go** (1.18+) | GC-shape stenciling + dictionaries | Unboxed | Via dictionary | One copy per *shape*; all pointer types share one copy |
| **C++** | Templates (heterogeneous) | Unboxed | No (unless RTTI) | At compile time; checked per instantiation |
| **Rust** | Monomorphization (heterogeneous) | Unboxed | Per copy (`TypeId`, `size_of`) | At compile time; checked once at the definition |

Java chose erasure for a reason that still matters to architects: **migration compatibility**. Generics arrived in Java
5 (2004), and erased generic code interoperates with pre-generics bytecode and "raw" types. That let an ecosystem that
already existed adopt generics one library at a time without recompiling everything. Rust had no such legacy, and chose
the other pole.

### 3. Rust code

**The boxing cost, measured.** Java's `List<Long>` stores a reference per element and a heap object per value. The
closest Rust equivalent is `Vec<Box<i64>>`. Here it is next to what Rust generics actually give you, `Vec<i64>`, with a
counting allocator (Chapter 3.2's pattern). Verified, release build:

```rust
// What Java's List<Long> costs, reproduced in Rust with Vec<Box<i64>>, next to Rust's Vec<i64>.
use std::hint::black_box;
use std::mem::size_of;

mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    static ALLOCS: AtomicUsize = AtomicUsize::new(0);
    static BYTES: AtomicUsize = AtomicUsize::new(0);
    pub struct Counting;
    // SAFETY: both methods forward their exact arguments to `System`, which upholds the
    // GlobalAlloc contract; the counters are plain atomics, so counting never allocates.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Relaxed);
            BYTES.fetch_add(layout.size(), Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }
    #[global_allocator]
    static GLOBAL: Counting = Counting;
    /// Runs `f` and returns (result, allocations, bytes requested).
    pub fn measure<R>(f: impl FnOnce() -> R) -> (R, usize, usize) {
        let (a0, b0) = (ALLOCS.load(Relaxed), BYTES.load(Relaxed));
        let r = f();
        (r, ALLOCS.load(Relaxed) - a0, BYTES.load(Relaxed) - b0)
    }
}

/// One generic function; both calls below get their own machine code.
fn total<T: Copy + Into<i64>>(xs: &[T]) -> i64 {
    xs.iter().map(|&x| x.into()).sum()
}

const N: i64 = 1_000_000;

fn main() {
    // Rust generics: Vec<i64> stores the numbers themselves, contiguously.
    let (flat, allocs, bytes) = counting::measure(|| (0..N).collect::<Vec<i64>>());
    println!("Vec<i64>      : {allocs:>7} allocations, {bytes:>8} bytes requested");

    // Erasure-style: every element is a separate heap object, the Vec holds pointers.
    let (boxed, allocs, bytes) = counting::measure(|| (0..N).map(Box::new).collect::<Vec<Box<i64>>>());
    println!("Vec<Box<i64>> : {allocs:>7} allocations, {bytes:>8} bytes requested");

    let small: Vec<i32> = (0..1000).collect();
    println!("total::<i64> = {}", total(black_box(&flat)));
    println!("total::<i32> = {}", total(black_box(&small)));
    println!("sum via boxes = {}", boxed.iter().map(|b| **b).sum::<i64>());
    println!("size_of::<i64>() = {}, size_of::<Box<i64>>() = {}", size_of::<i64>(), size_of::<Box<i64>>());
}
```

```text
Vec<i64>      :       1 allocations,  8000000 bytes requested
Vec<Box<i64>> : 1000001 allocations, 16000000 bytes requested
total::<i64> = 499999500000
total::<i32> = 499500
sum via boxes = 499999500000
size_of::<i64>() = 8, size_of::<Box<i64>>() = 8
```

**One allocation versus a million and one.** The boxed layout requests twice the bytes, because each 8-byte value sits
behind an 8-byte pointer. That is before the allocator's per-object overhead, and it is still *smaller* than Java's
version, because a Rust `Box<i64>` has no object header (§5). The generic `Vec<T>` simply stored `i64`s. No boxing
happened because nothing in Rust's generics requires a uniform representation.

**What Rust generic code can do with `T`, and Java's can't.** In Java, `new T()`, `new T[n]`, `T.class`, and a static
member accessed through `T` are all compile errors, because at run time there is no `T`. In Rust each instance knows its
`T`, so all of these work as long as a bound promises them (verified):

```rust
// Things Java generic code cannot do with T, because T is erased: here T is fully known in every copy.
use std::any::{type_name, TypeId};
use std::mem::{align_of, size_of};

/// A trait with an associated constant and a constructor: "static members" of T.
trait Currency: Copy + Default + std::fmt::Debug {
    const CODE: &'static str;
    const MINOR_UNITS: u32;
    fn from_minor(units: i64) -> Self;
}

#[derive(Clone, Copy, Default, Debug)]
struct Eur(i64);
#[derive(Clone, Copy, Default, Debug)]
struct Jpy(i64);

impl Currency for Eur {
    const CODE: &'static str = "EUR";
    const MINOR_UNITS: u32 = 2;
    fn from_minor(units: i64) -> Self {
        Eur(units)
    }
}
impl Currency for Jpy {
    const CODE: &'static str = "JPY";
    const MINOR_UNITS: u32 = 0;
    fn from_minor(units: i64) -> Self {
        Jpy(units)
    }
}

/// Java: `new T[n]` and `new T()` are illegal, and `T.CODE` does not exist. Rust: all fine.
fn ledger<C: Currency>(n: usize) -> Vec<C> {
    let mut rows = vec![C::default(); n]; // an array of T, filled with T's default
    rows[0] = C::from_minor(12_345); // a "constructor" called through the type parameter
    println!("{} ledger: {} rows, {} minor units, element = {}", C::CODE, rows.len(), C::MINOR_UNITS, type_name::<C>());
    rows
}

fn main() {
    let eur = ledger::<Eur>(3);
    let jpy = ledger::<Jpy>(2);
    println!("first rows: {} {} / {} {}", eur[0].0, Eur::CODE, jpy[0].0, Jpy::CODE);

    // Java: new ArrayList<String>().getClass() == new ArrayList<Integer>().getClass() is TRUE.
    // Rust: Vec<String> and Vec<u8> are different types, and the program can tell at run time.
    println!("TypeId Vec<String> == Vec<u8>? {}", TypeId::of::<Vec<String>>() == TypeId::of::<Vec<u8>>());
    println!("TypeId Vec<u8> == Vec<u8>?     {}", TypeId::of::<Vec<u8>>() == TypeId::of::<Vec<u8>>());
    println!(
        "layout: Option<u8> {}B/align {}, Option<u64> {}B/align {}, Option<Box<u64>> {}B",
        size_of::<Option<u8>>(),
        align_of::<Option<u8>>(),
        size_of::<Option<u64>>(),
        align_of::<Option<u64>>(),
        size_of::<Option<Box<u64>>>()
    );
}
```

```text
EUR ledger: 3 rows, 2 minor units, element = playground::Eur
JPY ledger: 2 rows, 0 minor units, element = playground::Jpy
first rows: 12345 EUR / 12345 JPY
TypeId Vec<String> == Vec<u8>? false
TypeId Vec<u8> == Vec<u8>?     true
layout: Option<u8> 2B/align 1, Option<u64> 16B/align 8, Option<Box<u64>> 8B
```

Three things here are impossible in Java. `C::CODE` reads a per-type constant through the type parameter.
`C::from_minor` and `C::default()` construct a `T` without a `Class<T>` token. And `vec![C::default(); n]` builds a
contiguous array of `T` values, not an array of references. The layout line shows the compiler choosing a
representation *per instance*. `Option<u8>` is 2 bytes. `Option<u64>` is 16. `Option<Box<u64>>` is 8, because `None`
uses the null pointer that a `Box` can never hold (Chapter 5.2, *Type Layout: Size, Alignment, Padding, and Niches*,
covers niches). A Java `Optional<Long>` is two heap objects in every case.

> **Why not `type_name` for logic?** [LANG] `std::any::type_name`'s output format is explicitly unspecified and may
> change between compiler versions. Use it for diagnostics only. `TypeId` is for identity checks, and even those are
> usually a sign that a trait method should be doing the work.

---

## Pass 2 · Systems level — *Bytecode, bridges, and per-type assembly*

### 4. Under the hood

**What `javac` does with a generic class.** Take the Java you'd write for a comparable money type and a generic `max`:

```java
final class Money implements Comparable<Money> {
    final long cents;
    Money(long cents) { this.cents = cents; }
    @Override public int compareTo(Money o) { return Long.compare(cents, o.cents); }
}

static <T extends Comparable<T>> T max(List<T> xs) {
    T best = xs.get(0);
    for (T x : xs) if (x.compareTo(best) > 0) best = x;
    return best;
}

Money top = max(payments);   // payments: List<Money>
```

`javac` performs **erasure** (JLS §4.6). Every use of `T` becomes its leftmost bound, here `Comparable`, and would be
`Object` for an unbounded `T`. The body is compiled once. `x.compareTo(best)` becomes an `invokeinterface` on
`Comparable.compareTo(Object)`. `xs.get(0)` returns `Object`, and at the *call site* in the caller, `javac` inserts a
`checkcast Money` so that `top` has the right type. The cast is the price of pretending the list holds `Money` when the
bytecode only knows `Object`.

Then there's the **bridge method**. `Money.compareTo(Money)` does not override `Comparable.compareTo(Object)`, whose
erased signature is different. To make the interface call reach it, `javac` generates a hidden method in `Money`:

```text
// What `javap -p Money.class` lists (representative, abridged; this book's toolchain has no JDK):
public int compareTo(Money);
public int compareTo(java.lang.Object);   // ACC_BRIDGE | ACC_SYNTHETIC: checkcast Money, then call compareTo(Money)
```

Every interface call from generic code lands on the bridge, which casts and then calls the real method. The bridge is
visible through reflection (`Method.isBridge()`), appears in stack traces, and is why a `ClassCastException` can be thrown
from a line of your code that contains no cast.

**What survives erasure.** [LIB] The class file keeps *declarations* in the `Signature` attribute. So
`Field.getGenericType()` can tell you that a field was declared `List<Money>`, and libraries such as Gson's `TypeToken`
and Jackson's `TypeReference` capture a generic type by subclassing (the "super type token" trick). Instances keep
nothing: an `ArrayList` object doesn't know what it holds.

**How the JIT recovers the performance.** [RUNTIME] HotSpot's interpreter and C1 tier record **type profiles** at
virtual and interface call sites and at `checkcast`s: which receiver classes have actually appeared. C2 then compiles
optimistically. At a site that has only seen `Money` (monomorphic), it inlines `Money.compareTo` behind a cheap class
check. Two classes (bimorphic) get two inlined paths. More than that, and the site is *megamorphic* and stays a real
virtual or interface dispatch. If a new class appears later, an **uncommon trap** deoptimizes the compiled code back to
the interpreter, and it is recompiled with the new profile. When it works, this matches much of what monomorphization
gives Rust, and it does so with facts that only exist at run time.

**Where it stops: profile pollution.** [RUNTIME] A type profile belongs to a *bytecode location*, not to a calling
context. `max` has one `compareTo` call site, shared by every caller in the process. If the service calls `max` on
`Money`, `Instant`, and `String`, that one site's profile records three classes and becomes megamorphic, for *all*
callers, including the hot one that only ever passes `Money`. C2 can still devirtualize when it inlines `max` into a
caller whose argument types it knows exactly, but only if `max` is small enough and shallow enough to be inlined there.
Library code that is widely shared, such as collections, streams, comparators, and frameworks, is exactly where profiles
are most polluted. Rust has no shared profile to pollute, because each caller's types produce their own instance of
`max`.

**What Rust does instead: per-type code, optimized per type.** Chapter 7.1 showed three instances of `largest` using
three different comparison instructions. The effect goes further than choosing instructions. Here is one generic `sum`
(library listing `ch02-03-per-type-codegen.rs`; verified release assembly, trimmed):

```rust,ignore
use std::ops::Add;

/// One source function. Each instantiation is optimized for its own T.
#[inline(never)]
pub fn sum<T: Copy + Default + Add<Output = T>>(xs: &[T]) -> T {
    let mut acc = T::default();
    for &x in xs {
        acc = acc + x;
    }
    acc
}

pub fn sum_u32(xs: &[u32]) -> u32 {
    sum(xs)
}
pub fn sum_f64(xs: &[f64]) -> f64 {
    sum(xs)
}
```

```text
playground::sum::<u32>:                               playground::sum::<f64>:
.LBB1_5:                                              .LBB0_9:
  movdqu xmm2, xmmword ptr [rdi + 4*rax]                addsd  xmm0, qword ptr [rax]
  paddd  xmm1, xmm2          ; 4 u32 adds at once       addsd  xmm0, qword ptr [rax + 8]
  movdqu xmm2, xmmword ptr [rdi + 4*rax + 16]           addsd  xmm0, qword ptr [rax + 16]
  paddd  xmm0, xmm2          ; 4 more, 2nd accumulator  addsd  xmm0, qword ptr [rax + 24]
  add    rax, 8                                         addsd  xmm0, qword ptr [rax + 32]
  cmp    r8, rax                                        addsd  xmm0, qword ptr [rax + 40]
  jne    .LBB1_5                                        addsd  xmm0, qword ptr [rax + 48]
  paddd  xmm0, xmm1          ; combine accumulators     addsd  xmm0, qword ptr [rax + 56]
  pshufd ...                 ; horizontal add           add    rax, 64
                                                        cmp    rax, rcx
                                                        jne    .LBB0_9
```

The `u32` instance is **vectorized**. It processes 8 elements per iteration in two SIMD accumulators and combines them
at the end. That is legal because integer addition (wrapping, in release) is associative, so the order doesn't matter.
The `f64` instance is unrolled 8 times but **strictly sequential**: one accumulator and one `addsd` after another,
because floating-point addition is *not* associative. Reordering it could change the result, and the compiler may not
do that without permission. Same source, different numeric semantics, different code. One generic body shared across
types could do neither.

**Static versus dynamic dispatch, in the same assembly.** The erased, Java-like strategy is available in Rust. You
choose it explicitly with `dyn Trait`. Here is one fee calculation, written both ways
(library listing `ch02-04-dyn-vs-generic.rs`; verified release assembly, trimmed):

```rust,ignore
pub trait Fee {
    fn fee_cents(&self, amount_cents: u64) -> u64;
}

pub struct Percent(pub u64); // basis points
impl Fee for Percent {
    fn fee_cents(&self, amount_cents: u64) -> u64 {
        amount_cents * self.0 / 10_000
    }
}

/// Static dispatch: one copy per F; the call to fee_cents is a direct call or inlined.
#[inline(never)]
pub fn total_fees_static<F: Fee>(fee: &F, amounts: &[u64]) -> u64 {
    amounts.iter().map(|&a| fee.fee_cents(a)).sum()
}

/// Dynamic dispatch: one copy for every implementor; each call goes through the vtable.
#[inline(never)]
pub fn total_fees_dyn(fee: &dyn Fee, amounts: &[u64]) -> u64 {
    amounts.iter().map(|&a| fee.fee_cents(a)).sum()
}
```

```text
playground::total_fees_static::<playground::Percent>:   playground::total_fees_dyn:
  mov    rcx, qword ptr [rdi]      ; basis points          mov  r13, qword ptr [rsi + 24]  ; fee_cents from the vtable
  movabs r9, 3777893186295716171   ; 1/10_000 as a      .LBB2_3:
.LBB0_5:                           ;   magic multiplier    mov  rsi, qword ptr [r14 + 8*rbp]
  mov    rax, qword ptr [rsi + 8*r10]                      mov  rdi, r12                   ; data pointer = &self
  imul   rax, rcx                  ; amount * bp           call r13                        ; indirect call, per element
  mul    r9                        ; ... / 10_000          add  r15, rax
  mov    r8, rdx                   ;   via multiply        inc  rbp
  shr    r8, 11                    ;   and shift           cmp  rbx, rbp
  add    r8, rdi                                           jne  .LBB2_3
  ...second element, unrolled 2x...
```

In the static instance, `fee_cents` has vanished. It was inlined, and the division by the constant 10,000 became a
multiplication by a "magic" reciprocal and a shift, a transformation that is only possible because the callee's body was
visible. The `dyn` version loads the function pointer from the vtable once (offset 24, after the drop, size, and align
entries; Chapter 6.4 describes the layout), then makes an **indirect call per element**. The optimizer cannot inline
through it or see the division. This is the Java model without the JIT: one shared body and dynamic calls, and nothing
at run time that will ever specialize it. Chapter 6.5, *Static vs Dynamic Dispatch: The Architect's Decision*, covers
when that trade is right.

**No bridges, no casts.** [RUSTC] Rust needs neither. A trait impl is looked up at compile time for each instance, so a
`Money: Ord` call in `largest::<Money>` goes straight to `<Money as Ord>::cmp`. There is no erased signature for a bridge
to adapt, and no `Object` to cast back from. Where Rust does erase, in `dyn Trait`, the vtable is built per (type,
trait) pair and holds exactly the right function pointers, so no cast is needed there either.

### 5. Memory

**Boxing, element by element.** For one million 64-bit values:

| Representation | Allocations | Bytes (payload + pointers) | Source |
|---|---|---|---|
| Rust `Vec<i64>` | 1 | 8,000,000 | Measured above |
| Rust `Vec<Box<i64>>` | 1,000,001 | 16,000,000 requested (+ allocator overhead) | Measured above |
| Java `long[]` | 1 | ~8,000,016 (array header + data) | Typical HotSpot layout |
| Java `ArrayList<Long>` | ~1,000,000 `Long`s + backing `Object[]` | ~4 MB of references + 16–24 MB of `Long` objects | Typical HotSpot layout, order of magnitude |

The Java rows are **typical**, not measured here (this book's toolchain has no JVM; measure with JOL on your JDK). On
64-bit HotSpot with compressed references, a reference is 4 bytes and a `Long` has an object header of 12 bytes plus 8
bytes of value, padded to 24 bytes. [VERSION] With compact object headers (JEP 519, a product option since JDK 25) the
header is 8 bytes and a `Long` fits in 16. `Long.valueOf` caches −128..127, so small values share objects. Either way,
the erased representation costs 2.5–3.5× the memory of the primitive array, plus a GC-traced object per element.

**The identity trap boxing creates.** [LANG] The JLS requires boxing to return cached objects for values in −128..127
(JLS §5.1.7), so `Integer a = 127, b = 127; a == b` is `true`, and with 128 it is typically `false`, because `==`
compares references. Rust has no boxing conversion, so it has no such trap. `==` on `T: PartialEq` always calls `eq`.

**Rust's representation is chosen per instance.** `Option<Box<u64>>` is 8 bytes because the compiler knows, for that
instance, that `Box` is never null. `Vec<Eur>` stores 8-byte `Eur` values contiguously. Nothing in the generic code
forces a pointer. The cost is the one Chapter 7.1 named: each instance is separate code.

### 6. CPU / OS

**Pointer chasing.** [CPU] Summing `Vec<i64>` streams through memory sequentially, and the hardware prefetcher keeps
up. Summing `Vec<Box<i64>>` or `List<Long>` loads a pointer, then dereferences it to wherever that object was
allocated. When the objects are scattered (after GC compaction or interleaved allocations), each element can cost a cache
miss. As an order-of-magnitude guide, an L1 hit is about 1 ns and a DRAM access about 100 ns (typical figures, not
measured here). That factor of up to ~100 per element dwarfs the cost of the addition. Part XX, *Performance
Engineering*, measures it.

**Vectorization needs contiguous primitives.** [CPU] The `paddd` loop above exists because `u32`s sit side by side. No
compiler can vectorize over a list of pointers to boxes. This is why the Java platform itself hand-specializes. It has
`IntStream`, `LongStream`, and `DoubleStream` next to `Stream<T>`, and `java.util.function` has over forty functional
interfaces, most of them primitive variants such as `IntUnaryOperator` and `ToLongFunction`. Libraries such as
fastutil and Eclipse Collections generate primitive collections for the same reason. **That is monomorphization, done by
hand, for a fixed list of types.** Rust's compiler does it for every type, including yours.

**Warm-up versus build time.** [RUNTIME] HotSpot specializes after the program has run long enough to profile, in
production, on every instance, and re-specializes after deoptimization. Rust specializes once, in CI. A freshly started
Rust process runs its final code immediately, which is why Chapter 1.4 counted warm-up as a real cost of the JVM for
autoscaled services. The Rust cost moved to the build (Chapter 7.3).

**GC.** [RUNTIME] A million `Long`s are a million objects for the collector to trace and possibly copy. A `Vec<i64>` is
one allocation that contains no pointers. With a counting allocator, as above, you can prove the difference in one run.

---

## Pass 3 · Architect level — *Choosing, porting, and recognizing erasure in Rust*

### 7. Trade-offs

| Property | Java erasure | Rust monomorphization |
|---|---|---|
| Code size | One body per generic method | One body per instantiation (Chapter 7.3) |
| Build time | Fast `javac`; optimization deferred to JIT | Slower: every instance is optimized by LLVM |
| Run-time speed of generic code | Good *after* warm-up when profiles are clean; boxing and polluted sites stay slow | Specialized from the first call; no boxing |
| Primitives | Boxed | Native |
| `T` at run time | Gone (declarations only, via reflection) | Known per instance (`size_of`, `TypeId`, associated consts) |
| Binary compatibility | Old and new code interoperate; raw types allowed | Generic code crosses crates as MIR; no stable ABI for generic code |
| Dynamic loading of new types | Easy: one body handles classes loaded later | Needs `dyn Trait` (or a plugin ABI) |
| Adaptivity | Re-optimizes to the observed workload | None at run time (PGO at build time, Part XX) |

The last three rows are real advantages of erasure, and they matter in architecture. A plugin system that loads
implementations at run time is natural on the JVM. In Rust, it is a `dyn Trait` boundary (Chapter 6.4) or a C-ABI
boundary (Part XVI), and generics stop there.

### 8. Java comparison

The consequences of erasure a Java engineer lives with, and what each becomes in Rust:

| Java consequence of erasure | Why | Rust |
|---|---|---|
| No `new T()`, `new T[n]`, `T.class` | No `T` at run time | `T::default()`, `vec![x; n]`, `TypeId::of::<T>()`, given a bound |
| `instanceof List<String>` is illegal | The list doesn't know its type argument | Types are never confused; `Any` exists for the rare dynamic case |
| `void f(List<String>)` and `void f(List<Integer>)` clash | Same erasure: "name clash" | No overloading at all; traits and generics instead (Chapter 6.1) |
| Bridge methods, `ClassCastException` from lines without a cast | Erased signatures must be adapted | No bridges; impls resolved per instance |
| Static fields shared by all parameterizations | One class | Associated consts are per type (`C::CODE`). A `static` can't mention a generic parameter, so per-type global state needs an explicit map (for example, keyed by `TypeId`) |
| Raw types and heap pollution | Migration compatibility | No raw types; the nearest equivalent is choosing `dyn Any` |
| Boxing and the `Integer` cache | `T` must be a reference | No boxing |
| Hand-specialized `IntStream` and primitive collections | No specialization | The compiler specializes everything |

**Heap pollution** deserves a sentence of its own, because it is the failure mode Rust's model removes. An unchecked
cast or a raw type lets an `Integer` into a `List<String>`. Nothing fails at the insertion. The failure is a
`ClassCastException` at some *later* `checkcast`, often in a different module, days later in a log. The type error and
the crash are separated in space and time. Rust types can't be polluted, because there is no unchecked path from a
value of one type into a container of another without `unsafe`.

**What the JIT does well, honestly.** For a monomorphic hot path, such as one comparator over one element type, C2
often produces code as good as Rust's instance, and it may inline things AOT compilation can't prove, such as a call
through an interface field that happens to hold one class in practice. The JIT's weaknesses are specific: boxing that
escape analysis can't remove (it rarely removes boxes stored in collections), polluted profiles in shared library code,
and warm-up.

**Valhalla, hedged.** [VERSION] Project Valhalla aims to close much of this gap. It adds *value classes* (JEP 401), which
can be flattened into arrays and fields without identity or headers, and later *specialized generics*, so that
`List<long>`-like code would not box. At the time of writing, JEP 401 has been in preview-track development, and
specialized generics are a later phase with no delivery date. Check the current JDK before assuming either. If
specialization does arrive, the JVM will do per-type specialization *at run time*, which is Rust's model moved from
`rustc` into the JIT.

**C# and Go, for calibration.** [RUNTIME] .NET generics are reified. The runtime keeps `T`, shares one compiled body
among all *reference-type* instantiations (they're all pointers), and compiles a separate body for each *value-type*
instantiation, so `List<int>` holds `int`s unboxed. It is a middle path: monomorphization where representation differs,
sharing where it doesn't. Go 1.18 made a similar split along different lines. It compiles one copy per **GC shape**
(all pointer types share one shape) and passes a dictionary for type-specific operations, so a method call on a
type-parameter value can become an indirect call. PlanetScale's engineering blog reported (2022) cases where Go generic
code was slower than hand-written or interface-based code for exactly this reason. Rust sits at the far end: every
instantiation gets its own code, including every pointer type.

> **Analogy limit.** "Rust generics are Java generics with the JIT's best case done at compile time" holds for hot,
> monomorphic code: direct calls, inlined bodies, no casts. It breaks in three places. **No boxing ever happens,** so
> Rust's best case is better than the JIT's best case for collections of numbers. **Nothing adapts at run time,** so a
> Rust instance can't exploit a type distribution it didn't know about at build time. **Nothing works across a dynamic
> boundary,** so a class loaded at run time has no Rust equivalent that generics can reach. You would use `dyn`, which is
> erasure you chose, with its cost visible in the code.

### 9. Production scenario

**Meridian's fraud feature extraction, ported.** The Java version (Chapter 1.2: 50K scores/s, p99 under 5 ms) had
learned to avoid its own generics. Feature vectors were `double[]`, ID sets were a primitive-collections `LongOpenHashSet`,
and a code-review rule banned `List<Long>` on the hot path. Each new feature type meant another hand-specialized
collection or another `IntFunction`-style interface. The Rust library that replaced it (called from Java through FFM,
Part XVI) uses the ordinary generic types, `HashSet<u64>` and `Vec<f64>`, and the per-type code comes for free. The
measurement that settled the design review was the one in §3. A million IDs stored the Java-generic way cost a million
and one allocations, and stored the Rust way cost one.

Two details surfaced during the port, and both came from per-type codegen. First, the team's generic `sum` over `u32`
counts vectorized, while the same function over `f64` scores stayed sequential (§4). That was the *correct* behavior for
floating point, and the team documented it instead of "fixing" it. Where order genuinely didn't matter, they wrote an
explicit chunked sum and accepted the rounding difference in a test with a tolerance. Second, the FFM boundary
had to be concrete. A C ABI has no generics, so the exported functions are `score_batch(ids: *const u64, n: usize, ...)`,
with generic Rust code behind them. Generics are a build-time feature, and they stop at every ABI.

### 10. Failure scenario

**Erasure, recreated by hand.** The risk-limits service (Chapter 1.2) was ported by a team fluent in Java. Its settings
had been a `Map<String, Object>` in Java, so the Rust port became `HashMap<&str, Box<dyn Any>>`, with a helper that
downcasts on read. One module stored a refund limit as `u32`, and the reader asked for `u64` (verified):

```rust
// A Java habit ported to Rust: erase everything to "Object" and cast back later.
use std::any::Any;
use std::collections::HashMap;

fn limit(settings: &HashMap<&str, Box<dyn Any>>, key: &str, default: u64) -> u64 {
    match settings.get(key).and_then(|v| v.downcast_ref::<u64>()) {
        Some(&v) => v,
        None => {
            println!("  {key}: missing or not a u64, using default {default}");
            default
        }
    }
}

fn main() {
    let mut settings: HashMap<&str, Box<dyn Any>> = HashMap::new();
    settings.insert("max_payout_cents", Box::new(500_000_u64));
    settings.insert("max_refund_cents", Box::new(20_000_u32)); // stored as u32 by another module
    println!("payout limit = {}", limit(&settings, "max_payout_cents", 0));
    println!("refund limit = {}", limit(&settings, "max_refund_cents", u64::MAX));
}
```

```text
payout limit = 500000
  max_refund_cents: missing or not a u64, using default 18446744073709551615
refund limit = 18446744073709551615
```

`downcast_ref::<u64>()` on a boxed `u32` returns `None`. That is correct behavior: `TypeId`s differ, as §3 showed. The
fallback turned "wrong type" into "no limit". The Java version would have thrown a `ClassCastException`, which is at
least loud. The Rust port quietly allowed unlimited refunds until a daily reconciliation job flagged the totals. This is
**heap pollution's failure mode, rebuilt voluntarily**: a type check moved from compile time to run time, and failed far
from its cause.

The fix put the types back where the compiler can see them. Settings became a typed struct deserialized once at
startup, `struct Limits { max_payout_cents: u64, max_refund_cents: u64 }`, with parsing errors reported at boot instead
of defaults at use. Where an open-ended registry was genuinely needed, the team used typed keys (a `Key<T>` carrying
its type in a `PhantomData`, Chapter 5.3), so that a key for `u64` can only fetch a `u64`. The review rule it produced:
**`dyn Any` in a Rust design is erasure you chose. Justify it like `unsafe`.**

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part VII).*

1. Explain homogeneous versus heterogeneous translation of generics. Which does Java use, which does Rust use, and why
   did Java choose as it did?
2. What is a bridge method? Show when `javac` generates one and what it does at run time.
3. Why can't Java generic code write `new T[n]`, and why can Rust's write `vec![T::default(); n]`? What must the Rust
   signature say?
4. How does HotSpot recover performance in erased generic code? Define type profile, monomorphic/bimorphic/megamorphic
   site, uncommon trap, and profile pollution.
5. The same generic `sum` vectorizes for `u32` and not for `f64`. Explain why, and why a single shared body could do
   neither.
6. Compare .NET's and Go's generics implementations with Java's and Rust's. Where does each one share code, and where
   does each specialize?
7. What is heap pollution, and what is its closest Rust equivalent? How would you spot that equivalent in code review?
8. List three things erasure makes easy that monomorphization makes hard, and the Rust mechanism for each.

### 12. Exercises

- **Beginner.** Change `ch02-01-boxing-cost.rs` to measure `Vec<Option<i64>>` and `Vec<Option<Box<i64>>>`. Predict
  the allocation counts and bytes first.
- **Intermediate.** Write `fn parse_all<T: FromStr>(items: &[&str]) -> Vec<Option<T>>` and call it with three types.
  Then write the Java signature that would do the same, and list every place the Java version needs a `Class<T>` or a
  cast.
- **Advanced.** Add `sum::<i64>` and `sum::<f32>` to `ch02-03-per-type-codegen.rs`. Predict which will vectorize,
  then emit the assembly and check. Then write a `sum_f64_unordered` that vectorizes by using several accumulators, and
  quantify the rounding difference on a test input.
- **Systems.** On a JVM you have access to, measure `ArrayList<Long>` against `long[]` for one million elements with JOL
  (layout) and a heap histogram. Compare your numbers with the typical ones in §5, and note your JDK version and flags.
- **Architecture.** Identify one place in a Java system you know where erasure forced a design: a `Class<T>` parameter,
  a type token, a primitive collection, or a bridge-method surprise. Sketch the Rust equivalent and state what it costs
  in build time and binary size.

### 13. Debugging exercise

A Java engineer's first Rust generic:

```rust,compile_fail
/// A Java habit: "give me a fresh T". In Rust the bound must say T can be constructed.
fn fresh_batch<T>(n: usize) -> Vec<T> {
    (0..n).map(|_| T::new()).collect()
}

fn main() {
    let batch: Vec<String> = fresh_batch(3);
    println!("{}", batch.len());
}
```

1. Predict the error code and message. How does it differ from Java's "cannot instantiate the type T"?
2. `String::new` exists. Why doesn't the compiler use it for `fresh_batch::<String>`?
3. Fix it two ways: with a standard-library bound, and with your own trait. Which one lets a type without a `Default`
   participate?

### 14. Design exercise

**A generic cache for Java and Rust callers.** Meridian's platform team wants one in-process cache library, written in
Rust. Rust services will use it directly, and Java services will call it through FFM. Rust callers want `Cache<K, V>`
with arbitrary key and value types. Java callers can pass only what a C ABI allows: integers, pointers, and lengths.

Design the layering. Which part is generic and monomorphized for Rust callers? Which part is concrete at the FFM
boundary (for example, byte-string keys and values), and how do Java callers get typed access without erasure's pitfalls?
How many instances of the core does each Rust service compile? What happens to the "no boxing" advantage at the Java
boundary? Name the measurement that would make you choose a single concrete `Cache<Vec<u8>, Vec<u8>>` for everyone.
