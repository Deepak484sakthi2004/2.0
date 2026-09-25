# Chapter 6.4 — Trait Objects, vtables, and dyn Compatibility

> **Where this sits:** Part VI · Traits · chapter 4 of 5
> **Prerequisites:** Chapters 6.1–6.3. Chapter 4.4 (variance), Chapter 1.3 (`Send`/`Sync` introduced).
> **After this chapter you can:** build heterogeneous collections with `dyn Trait`; draw a fat pointer and a vtable from
> real compiler output (verified LLVM IR and assembly); explain every dyn-compatibility rule from what a vtable can hold,
> and fix E0038 without giving up the API you wanted; use `Any` for downcasting and avoid its classic trap; add `Send` and
> `Sync` to trait objects deliberately; and explain why a trait object's generic arguments are invariant.

---

## Pass 1 · User level — *A value whose type is "some implementor"*

### 1. Problem

Meridian's gateway configures middleware per route, from a config file: `auth, ratelimit=500, tenant`. The set and
order of middleware are known only when the config is read, at run time. Chapter 6.1's tools can't express that.
`Vec<T>` holds one `T`, and `fn run<M: Middleware>(m: &M)` is compiled per type *before* the program starts.

What's needed is a type that means "some type implementing `Middleware`, I don't know which," so that
`Vec<Box<dyn Middleware>>` can hold an `Auth`, a `RateLimit`, and a `TenantTag` side by side. That's a **trait
object**, and it's Rust's dynamic dispatch: the Java default, available in Rust on request.

### 2. Mental model

`dyn Middleware` is a type: *some* implementor, with its concrete identity erased. Its size isn't known at compile
time (an `Auth` is 0 bytes, a `RateLimit` 4), so it's **unsized** and always lives behind a pointer: `&dyn M`,
`&mut dyn M`, `Box<dyn M>`, `Rc<dyn M>`, `Arc<dyn M>`. That pointer is **fat**: two words.

```text
 Box<dyn Shape>  (16 bytes)                                   vtable for <Label as Shape>   (static, read-only)
 ┌──────────────┬───────────────┐                             ┌─────────────────────────────┐
 │ data ptr ─┐  │ vtable ptr ───┼───────────────────────────► │ +0   drop glue for Label    │
 └───────────┼──┴───────────────┘                             │ +8   size  = 40             │
             ▼                                                │ +16  align = 8              │
   heap: Label { text: String, w: f64, h: f64 }               │ +24  <Label as Shape>::area │
                                                              │ +32  <Label as Shape>::name │
                                                              └─────────────────────────────┘
```

A **vtable** exists once per (concrete type, trait) pair and is shared by every object of that type. A call
`shape.area()` becomes: load the function pointer at `vtable + 24`, call it with the data pointer as `self`. The first
three entries (drop, size, align) are what the compiler needs to *drop and deallocate* an object whose type it no
longer knows. [RUSTC] The exact layout is an implementation detail, not a language guarantee. §4 reads it out of real
compiler output.

**Dyn compatibility** (formerly "object safety"; the diagnostics now say "dyn compatible" [VERSION]) is the question
"can this trait be turned into a vtable?" Every method must be callable given only a data pointer and a table of
function pointers. §4 derives the rules from that one sentence.

**Extra bounds ride along in the type.** `dyn Middleware + Send + Sync` is a *different type* from `dyn Middleware`.
Only **auto traits** (`Send`, `Sync`, `Unpin`, …) may be added this way, plus a lifetime. [LANG] Defaults apply when
you write no lifetime: `Box<dyn Trait>` means `Box<dyn Trait + 'static>`, and `&'a dyn Trait` means
`&'a (dyn Trait + 'a)`.

### 3. Rust code

Sizes of trait-object pointers, and what `size_of_val` reads from the vtable (verified):

```rust
use std::mem::{align_of_val, size_of, size_of_val};
use std::rc::Rc;
use std::sync::Arc;

trait Shape {
    fn area(&self) -> f64;
    fn name(&self) -> &'static str;
}

struct Circle {
    r: f64,
}
struct Rect {
    w: f64,
    h: f64,
}
struct Tag(u8);

impl Shape for Circle {
    fn area(&self) -> f64 {
        std::f64::consts::PI * self.r * self.r
    }
    fn name(&self) -> &'static str {
        "circle"
    }
}
impl Shape for Rect {
    fn area(&self) -> f64 {
        self.w * self.h
    }
    fn name(&self) -> &'static str {
        "rect"
    }
}
impl Shape for Tag {
    fn area(&self) -> f64 {
        0.0
    }
    fn name(&self) -> &'static str {
        if self.0 > 0 { "tag" } else { "empty-tag" }
    }
}

fn main() {
    println!("{:<30}{:>3} bytes", "&Circle", size_of::<&Circle>());
    println!("{:<30}{:>3} bytes", "&[u8]", size_of::<&[u8]>());
    println!("{:<30}{:>3} bytes", "&dyn Shape", size_of::<&dyn Shape>());
    println!("{:<30}{:>3} bytes", "&mut dyn Shape", size_of::<&mut dyn Shape>());
    println!("{:<30}{:>3} bytes", "*const dyn Shape", size_of::<*const dyn Shape>());
    println!("{:<30}{:>3} bytes", "Box<dyn Shape>", size_of::<Box<dyn Shape>>());
    println!("{:<30}{:>3} bytes", "Option<Box<dyn Shape>>", size_of::<Option<Box<dyn Shape>>>());
    println!("{:<30}{:>3} bytes", "Rc<dyn Shape>", size_of::<Rc<dyn Shape>>());
    println!("{:<30}{:>3} bytes", "Arc<dyn Shape + Send + Sync>", size_of::<Arc<dyn Shape + Send + Sync>>());

    // A fat pointer's first word is the ordinary data pointer.
    let c = Circle { r: 1.0 };
    let fat: &dyn Shape = &c;
    let data = fat as *const dyn Shape as *const ();
    println!("data half == &c: {}", data == &c as *const Circle as *const ());

    // size_of_val / align_of_val on a dyn value read the size and align entries of the vtable.
    let shapes: Vec<Box<dyn Shape>> =
        vec![Box::new(Circle { r: 1.0 }), Box::new(Rect { w: 2.0, h: 3.0 }), Box::new(Tag(7))];
    for s in &shapes {
        println!(
            "{:<7} area={:>6.3}  size_of_val={:>2}  align_of_val={}",
            s.name(),
            s.area(),
            size_of_val(&**s),
            align_of_val(&**s)
        );
    }
}
```

```text
&Circle                         8 bytes
&[u8]                          16 bytes
&dyn Shape                     16 bytes
&mut dyn Shape                 16 bytes
*const dyn Shape               16 bytes
Box<dyn Shape>                 16 bytes
Option<Box<dyn Shape>>         16 bytes
Rc<dyn Shape>                  16 bytes
Arc<dyn Shape + Send + Sync>   16 bytes
data half == &c: true
circle  area= 3.142  size_of_val= 8  align_of_val=8
rect    area= 6.000  size_of_val=16  align_of_val=8
tag     area= 0.000  size_of_val= 1  align_of_val=1
```

Every pointer to a `dyn Shape` is two words, like a slice pointer: slices carry a *length* as metadata, trait objects
a *vtable*. Adding `Send + Sync` doesn't change the size, because auto traits have no methods and add no vtable entries.
`Option<Box<dyn Shape>>` stays 16 bytes, because the data pointer can't be null, so `None` uses that niche
(Chapter 2.6). And the same `size_of_val(&**s)` expression returns 8, 16, and 1 for three elements of one `Vec`: the
answer comes from each object's vtable at run time.

---

## Pass 2 · Systems level — *The vtable, from the compiler's own output*

### 4. Under the hood

**The vtable in LLVM IR.** Listing `ch04-03-vtable.rs` defines two shapes: `Circle { r: f64 }`, with nothing to
destroy, and `Label { text: String, w: f64, h: f64 }`, which owns heap memory. Two functions return `Box<dyn Shape>`.
rustc 1.98.1, release build (symbols are v0-mangled in the IR; demangled by hand here, attributes removed):

```text
; <Label as Shape> vtable
@vtable.0 = private unnamed_addr constant <{ ptr, [16 x i8], ptr, ptr }> <{
    ptr @"core::ptr::drop_glue::<Label>",
    [16 x i8] c"(\00\00\00\00\00\00\00\08\00\00\00\00\00\00\00",
    ptr @"<Label as Shape>::area",
    ptr @"<Label as Shape>::name" }>, align 8

; <Circle as Shape> vtable
@vtable.1 = private unnamed_addr constant <{ [24 x i8], ptr, ptr }> <{
    [24 x i8] c"\00\00\00\00\00\00\00\00\08\00\00\00\00\00\00\00\08\00\00\00\00\00\00\00",
    ptr @"<Circle as Shape>::area",
    ptr @"<Circle as Shape>::name" }>, align 8
```

Read `@vtable.0` as five 8-byte words: a pointer to `Label`'s **drop glue**, then `size = 0x28` (the byte `(` is ASCII
0x28, so 40: a 24-byte `String` plus two `f64`s), `align = 8`, then the two methods in declaration order.
`@vtable.1` is more interesting. Its first 24 bytes are plain data: **eight zero bytes where the drop pointer would
be**, then size 8 and align 8. [RUSTC] Current rustc stores a null drop entry for types with no drop glue, and dropping
a `Box<dyn Shape>` skips the call. That's an implementation detail you can observe, not one you may rely on.

The **unsizing coercion**, `Box<Circle>` to `Box<dyn Shape>`, is visible in `circle()` (trimmed):

```text
define { ptr, ptr } @"circle"(double %r) {
  %0 = call ptr @__rust_alloc(i64 8, i64 8)             ; Box::new: 8 bytes, align 8
  store double %r, ptr %0
  %2 = insertvalue { ptr, ptr } poison, ptr %0, 0       ; word 0: the data pointer
  %3 = insertvalue { ptr, ptr } %2, ptr @vtable.1, 1    ; word 1: the ADDRESS OF A CONSTANT
  ret { ptr, ptr } %3
}
```

Making a trait object costs nothing beyond the allocation you'd make anyway. It's pairing a pointer with a link-time
constant. The type information that "disappears" was turned into that constant's address.

**The dynamic call in assembly.** Listing `ch04-02-dyn-call.rs` sums areas two ways: `total_area_dyn(&[Box<dyn
Shape>])` and a generic `total_area_static<T: Shape>(&[T])`, instantiated for `Circle` and `Square`. Release build
(comments added):

```text
playground::total_area_dyn:
	...
.LBB2_4:                                  ; loop over 16-byte fat pointers
	movsd	qword ptr [rsp], xmm0         ; SPILL the running total to the stack
	mov	rdi, qword ptr [r14]              ; data pointer   → first argument (self)
	mov	rax, qword ptr [r14 + 8]          ; vtable pointer
	call	qword ptr [rax + 24]          ; INDIRECT CALL: vtable slot 3 = Shape::area
	movsd	xmm1, qword ptr [rsp]         ; RELOAD the total
	addsd	xmm1, xmm0
	movsd	qword ptr [rsp], xmm1
	movsd	xmm0, qword ptr [rsp]
	add	r14, 16                           ; next fat pointer
	cmp	r14, rbx
	jne	.LBB2_4

playground::total_area_static::<playground::Square>:
	...
.LBB1_9:                                  ; unrolled ×8, no calls at all
	movsd	xmm1, qword ptr [rax]
	movsd	xmm2, qword ptr [rax + 8]
	mulsd	xmm1, xmm1                    ; side * side, inlined
	addsd	xmm1, xmm0
	mulsd	xmm2, xmm2
	addsd	xmm2, xmm1
	...                                   ; six more elements
	add	rax, 64
	cmp	rax, rcx
	jne	.LBB1_9

playground::circles:
	jmp	qword ptr [rip + playground::total_area_static::<playground::Circle>@GOTPCREL]
```

Three facts are on the page:

1. **`call qword ptr [rax + 24]`** is the vtable call. Offset 24 skips drop, size, and align, so `area` is the first
   method slot, exactly as in the IR.
2. **The spill.** The x86-64 System V ABI makes every XMM register caller-saved, and the compiler can't see what
   `area` does, so the running total goes to the stack and comes back around *every* call. The static version keeps it
   in registers and unrolls by 8. The call's cost isn't just the call itself. It's everything the optimizer couldn't do
   across it.
3. **One dyn function, two static ones.** `total_area_dyn` exists once and serves every `Shape`.
   `total_area_static::<Circle>` and `::<Square>` are separate machine code. That's the code-size side of the trade
   (Chapter 6.5, and Part VII, Chapter 7.3).

**The dyn-compatibility rules, derived from the vtable.** [LANG] A trait can be a `dyn` type only if a finite table of
function pointers, each called with an erased `self`, can implement every method:

| Rule | Why the vtable can't cope otherwise |
|---|---|
| No generic methods (`fn record<M: Debug>(&self, m: M)`) | Monomorphization makes a *family* of functions, one per `M`. Which ones go in the table? All possible `M`s are unknowable when the vtable is built |
| No `Self` by value in arguments or return (`fn duplicate(&self) -> Self`) | The caller must reserve space for the return value, and through `dyn` it doesn't know the size |
| `Sized` must not be a supertrait (`trait M: Clone`, since `Clone: Sized`) | A `dyn M` is by definition unsized, so a trait requiring `Sized` can't describe it |
| Receiver must be a pointer to `Self` (`&self`, `&mut self`, `Box<Self>`, `Rc<Self>`, `Arc<Self>`, `Pin<&mut Self>`) | The data pointer *is* the receiver. A by-value `self` would need the size |
| No associated consts; no associated functions without a receiver (`fn new() -> Self`) | There's no `self`, so no vtable to look them up in |
| No `async fn` or return-position `impl Trait` in the trait | Each impl returns a *different* hidden type (Part XII) |

**The escape hatch: `where Self: Sized`.** A method with this bound is excluded from the vtable. It remains callable on
concrete types and is simply unavailable through `dyn`. The trait stays dyn-compatible (§10 uses it).

**Upcasting and downcasting.** [VERSION] Since Rust 1.86, `&dyn Sub` coerces to `&dyn Super` when `trait Sub: Super`
(*trait upcasting*). [RUSTC] To support it, a vtable carries what's needed to produce the supertrait's vtable pointer.
In current rustc, the first supertrait's entries form a prefix and others are reached through stored pointers (again,
an implementation detail). *Downcasting* goes through `std::any::Any`: `Any::type_id` is a vtable method returning a
`TypeId`, and `downcast_ref::<T>()` compares it with `TypeId::of::<T>()`. `Any` requires `'static`, because `TypeId`
doesn't distinguish lifetimes, and a downcast that ignored them could forge a longer lifetime.

**Variance: the promise from Chapter 4.4.** [LANG] A trait object's **lifetime bound is covariant**, and its **generic
arguments are invariant**. Listing `ch04-10-dyn-variance.rs`:

```rust,compile_fail
trait Sink<T> {
    fn put(&mut self, item: T);
}

struct Collect<T>(Vec<T>);

impl<T> Sink<T> for Collect<T> {
    fn put(&mut self, item: T) {
        self.0.push(item);
    }
}

/// Allowed: the object's lifetime BOUND (`+ 'static` -> `+ 'a`) can shrink.
fn shorten_bound<'a>(s: Box<dyn Sink<u32> + 'static>) -> Box<dyn Sink<u32> + 'a> {
    s
}

/// Rejected: the trait's generic ARGUMENTS are invariant; &'static str -> &'a str is not allowed.
fn shorten_arg<'a>(s: Box<dyn Sink<&'static str>>) -> Box<dyn Sink<&'a str>> {
    s
}

fn main() {
    let mut s: Box<dyn Sink<&'static str>> = Box::new(Collect(Vec::new()));
    s.put("static text");
    let _shorter = shorten_arg(s);
    let _bounded = shorten_bound(Box::new(Collect(Vec::new())));
}
```

```text
error: lifetime may not live long enough
20 | fn shorten_arg<'a>(s: Box<dyn Sink<&'static str>>) -> Box<dyn Sink<&'a str>> {
   |                -- lifetime `'a` defined here
21 |     s
   |     ^ returning this value requires that `'a` must outlive `'static`
```

`shorten_bound` compiles and `shorten_arg` doesn't. The reason is Chapter 4.4's `&mut` argument again. The compiler
can't see which methods of `Sink<T>` take `T` as *input* (like `put`), so it must assume some do. If
`dyn Sink<&'static str>` could become `dyn Sink<&'a str>`, you could `put` a short-lived `&'a str` into a
`Collect<&'static str>`, whose owner later reads it as `'static`, and it would dangle. Invariance is the only safe
default for an unknown mix of inputs and outputs.

### 5. Memory

A heterogeneous pipeline, `Vec<Box<dyn Middleware>>`, is three kinds of memory:

```text
 Vec buffer (heap, contiguous)       objects (heap, one allocation each)      vtables (read-only data, one per type)
 ┌────────────┬────────────┐
 │ data ──────┼─ vtable ───┼──────────────────────────────────────────────► <Auth as Middleware>
 ├────────────┼────────────┤         (Auth is zero-sized: no allocation)
 │ data ──────┼─ vtable ───┼───────► RateLimit { per_sec }  ─ ─ ─ ─ ─ ─ ─ ► <RateLimit as Middleware>
 ├────────────┼────────────┤
 │ data ──────┼─ vtable ───┼───────► TenantTag  ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ► <TenantTag as Middleware>
 └────────────┴────────────┘
   16 bytes per element
```

- **Per element:** 16 bytes in the vector, plus a separate heap allocation for each non-zero-sized object, plus
  allocator overhead per allocation [LIB]. `Box::new` of a zero-sized type like `Auth` doesn't allocate.
- **Per type:** one vtable, shared. Its size is 3 words plus one per method, plus any supertrait bookkeeping.
- **Locality:** iterating the vector touches the buffer sequentially. Each element's object is somewhere else on the
  heap, and each vtable is somewhere in `.rodata`. With a few long-lived middleware, it's all cache-resident and
  irrelevant. With a million small objects, it's a pointer chase per element (Chapter 6.5 measures it).

Two identity subtleties:

- [RUSTC] The **same** (type, trait) pair can end up with **two** vtable copies, for example from different codegen units.
  So comparing two `&dyn Trait` with `ptr::eq` compares vtable addresses too, and may say "different" for the same
  object. `std::ptr::addr_eq` (stable since 1.76) compares only the data addresses [VERSION].
- A `Box<dyn Trait>` holding a **zero-sized** value has a dangling (non-null, aligned) data pointer, and distinct ZST
  objects may share an address. Never use data addresses of trait objects as identity keys when ZSTs are possible.

### 6. CPU / OS

**What an indirect call costs** (order of magnitude, x86-64):

- **Two dependent loads** to find the target: the vtable pointer (often already in a register) and the function pointer
  at `[vtable + offset]`, usually an L1 hit for hot vtables.
- **An indirect branch.** The CPU's indirect-branch predictor guesses the target from history. When one call site sees
  one target, or a repeating pattern, the prediction is nearly free. When targets vary unpredictably, each mispredict
  flushes the pipeline, on the order of 15–20 cycles on current cores.
- **Lost optimization**, often the biggest item: no inlining, so no constant propagation, no vectorization, and spills
  like the one in the assembly above. That's why the static loop was unrolled ×8 while the dynamic one reloaded its
  accumulator from the stack every iteration.

**Devirtualization** happens when LLVM can see the vtable constant, for example when an object is created and used
within one function after inlining. Then the indirect call becomes direct, sometimes inlined. It's an optimization, not
a guarantee [RUSTC]. Profile-guided optimization can also promote hot indirect calls to guarded direct calls (Part XX).

**The OS angle.** Indirect branches are a Spectre variant 2 attack surface, and kernels compile them with mitigations
(retpolines, or hardware controls such as IBRS) that make each one noticeably more expensive [OS]. Ordinary user-space
Rust doesn't pay that tax by default. It's one reason kernel developers work to devirtualize hot paths.

---

## Pass 3 · Architect level — *Designing traits that can be objects*

### 7. Trade-offs

**Which pointer?**

| Form | Owns | Use when |
|---|---|---|
| `&dyn T` / `&mut dyn T` | No | Callbacks and parameters: "any implementor, for this call". No allocation |
| `Box<dyn T>` | Yes, uniquely | Heterogeneous collections, config-built pipelines, returning "some implementor" |
| `Arc<dyn T + Send + Sync>` | Shared, across threads | Plugins and services shared by worker threads |
| `Rc<dyn T>` | Shared, one thread | Single-threaded graphs of handlers |

**Designing a dyn-compatible trait:**

- **Keep the object-facing trait small.** Only what callers through `dyn` need.
- **Put generic conveniences in an extension trait with a blanket impl over `?Sized`**, so trait objects get them too.
  Listing `ch04-11-ext-trait.rs` (verified) gives `dyn Middleware` a generic `record<M: Debug>`:

  ```rust,ignore
  trait Middleware {
      fn name(&self) -> &'static str;
      fn handle(&self, path: &str) -> bool;
  }

  trait MiddlewareExt: Middleware {
      fn record<M: Debug>(&self, metric: M) {
          println!("[{}] metric {metric:?}", self.name());
      }
  }

  impl<T: Middleware + ?Sized> MiddlewareExt for T {}

  /// Compile-time guard: this stops compiling (E0038) if anyone makes Middleware dyn-incompatible.
  fn _assert_dyn_compatible(_: &dyn Middleware) {}
  ```

  ```text
  [auth] metric ("admitted", true)
  [tenant] metric ("admitted", true)
  [auth] metric 42
  ```

  The generic method is monomorphized at each *call site*, through the object's `name()` vtable entry. It never needs a
  vtable slot of its own.
- **Replace `Clone` with `clone_box(&self) -> Box<dyn Trait>`**, and `-> Self` constructors with factories (§9).
- **Guard the property in CI.** The `_assert_dyn_compatible` function fails to compile the moment someone adds a generic
  method, turning a downstream break into a local one.

**`Any` downcasting is a design smell with legitimate uses.** Configuration inspection (§9), error chains
(`Error::downcast_ref`, Part VIII), and test assertions are fine. Business logic that downcasts to decide behavior has
reinvented `match` on an enum, badly. Chapter 6.5 makes the enum-or-`dyn` decision explicit.

### 8. Java comparison

**Fat pointer, thin object vs thin pointer, fat object.** Every Java object carries a header with a class pointer, and
through it the vtable and interface tables. Every reference is one word, and every call on an interface type is
virtual unless the JIT proves otherwise. Rust inverts the placement. Objects carry nothing, and the vtable pointer
travels in the *reference*, only when you ask for `dyn`:

| | Java | Rust |
|---|---|---|
| Where the dispatch table pointer lives | Object header (every object) | Fat pointer (only `dyn` references) |
| Cost when dynamic dispatch isn't used | Header on every object, anyway | None |
| Interface call mechanism | `invokeinterface`: itable lookup, sped up by inline caches | `call [vtable + k]`: fixed offset, no search |
| Devirtualization | JIT, from run-time profiles: monomorphic and bimorphic inline caches, class-hierarchy analysis, deoptimization if wrong | Static only (LLVM sees the type), or PGO promotion (Part XX). No speculation, no deopt |
| Generic methods in interfaces | Fine: erasure makes one method taking `Object` | Not in the vtable: monomorphization makes many methods |
| Downcasting | `instanceof`, casts, `ClassCastException` | `Any::downcast_ref` returning `Option`, `'static` types only |

The generic-methods row is the deep reason Rust has dyn-compatibility rules and Java doesn't. **Erasure** compiles a
generic method *once*, for `Object`, so it fits in a table. **Monomorphization** compiles it *per type*, so it doesn't.
Part VII, Chapter 7.2 develops the trade-off.

**Use-site vs declaration-site variance (PECS).** Java decides variance per *use* with wildcards: `List<? extends
Number>` is a producer you read from (covariant), `List<? super Integer>` a consumer you write to (contravariant). That's
Bloch's "producer extends, consumer super." Rust has no wildcards. A struct's variance is inferred from its fields at the
declaration (Chapter 4.4), and a trait object's type arguments are always invariant (§4), because the compiler can't see
which methods consume and which produce. The Rust answer to "I need a `Sink` of a wider type here" is usually a generic
parameter instead of an object: `fn feed<S: Sink<T>>(s: &mut S, …)` is instantiated for the exact types at each call
site, so there's nothing to convert.

**JIT inline caches vs Rust's static choice.** HotSpot starts every interface call as dynamic, then *profiles*. A call
site that sees one receiver class becomes a guarded direct call that C2 can inline, two classes become a bimorphic
check, and more becomes a *megamorphic* table dispatch. Java code is dynamic by default and made static by
speculation. Rust code is static by default and dynamic by declaration, with no speculation in either direction.

> **Analogy limit.** "`Box<dyn Trait>` is a Java interface reference" holds for dispatch and misses ownership,
> sendability, and lifetimes. A `Box<dyn Middleware>` *owns* its object, and dropping the box runs the drop glue from
> the vtable. It isn't `Send` unless the type says `+ Send`. It carries a lifetime bound (`'static` by default). A Java
> interface reference is shared, always sendable (thread-safety is your problem), and keeps the object alive as long as
> it exists.

### 9. Production scenario

**Meridian's config-driven middleware pipeline.** The gateway builds a pipeline per route from strings like
`"auth, ratelimit=500, tenant"`. A registry maps names to factory functions. The trait is designed for `dyn` from the
start: a generic metrics method excluded with `where Self: Sized`, `clone_box` instead of `Clone`, and `Any` as a
supertrait so operators can inspect the configured middleware (listing `ch04-06-middleware-pipeline.rs`, verified):

```rust
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;

#[derive(Debug, Default)]
struct Request {
    path: String,
    headers: Vec<(String, String)>,
    tenant: Option<String>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// `Any` as a supertrait lets callers upcast &dyn Middleware to &dyn Any and downcast (Rust 1.86+).
trait Middleware: Any {
    fn name(&self) -> &'static str;
    fn handle(&self, req: &mut Request) -> Result<(), String>;

    /// Generic method kept OUT of the vtable: callable only on concrete (Sized) types.
    fn record<M: Debug>(&self, metric: M)
    where
        Self: Sized,
    {
        println!("  [{}] metric {metric:?}", self.name());
    }

    /// The dyn-compatible replacement for a `Clone` supertrait: returns a trait object, not `Self`.
    fn clone_box(&self) -> Box<dyn Middleware>;
}

#[derive(Clone)]
struct Auth;

#[derive(Clone)]
struct RateLimit {
    per_sec: u32,
}

#[derive(Clone)]
struct TenantTag;

impl Middleware for Auth {
    fn name(&self) -> &'static str {
        "auth"
    }
    fn handle(&self, req: &mut Request) -> Result<(), String> {
        req.header("authorization").map(|_| ()).ok_or_else(|| "401 missing credentials".to_string())
    }
    fn clone_box(&self) -> Box<dyn Middleware> {
        Box::new(self.clone())
    }
}

impl Middleware for RateLimit {
    fn name(&self) -> &'static str {
        "ratelimit"
    }
    fn handle(&self, _req: &mut Request) -> Result<(), String> {
        Ok(()) // the real limiter (Chapter 6.1) would try_acquire here
    }
    fn clone_box(&self) -> Box<dyn Middleware> {
        Box::new(self.clone())
    }
}

impl Middleware for TenantTag {
    fn name(&self) -> &'static str {
        "tenant"
    }
    fn handle(&self, req: &mut Request) -> Result<(), String> {
        req.tenant = req.header("x-tenant").map(str::to_string);
        Ok(())
    }
    fn clone_box(&self) -> Box<dyn Middleware> {
        Box::new(self.clone())
    }
}

/// A factory turns the config argument into a boxed middleware. Plain fn pointers: no captures needed.
type Factory = fn(&str) -> Box<dyn Middleware>;

fn registry() -> HashMap<&'static str, Factory> {
    let mut r: HashMap<&'static str, Factory> = HashMap::new();
    r.insert("auth", |_| Box::new(Auth));
    r.insert("ratelimit", |arg| Box::new(RateLimit { per_sec: arg.parse().unwrap_or(100) }));
    r.insert("tenant", |_| Box::new(TenantTag));
    r
}

/// Build the pipeline from a config string such as "auth, ratelimit=500, tenant".
fn build(config: &str, reg: &HashMap<&'static str, Factory>) -> Result<Vec<Box<dyn Middleware>>, String> {
    config
        .split(',')
        .map(|entry| {
            let (name, arg) = entry.split_once('=').unwrap_or((entry, ""));
            let name = name.trim();
            reg.get(name).map(|make| make(arg.trim())).ok_or_else(|| format!("unknown middleware {name:?}"))
        })
        .collect()
}

fn run(pipeline: &[Box<dyn Middleware>], req: &mut Request) -> Result<(), String> {
    for m in pipeline {
        m.handle(req).map_err(|e| format!("{}: {e}", m.name()))?;
    }
    Ok(())
}

fn main() {
    let reg = registry();
    let pipeline = build("auth, ratelimit=500, tenant", &reg).unwrap();
    let names: Vec<&str> = pipeline.iter().map(|m| m.name()).collect();
    println!("pipeline = {names:?}");

    let mut req = Request {
        path: "/v1/payments".into(),
        headers: vec![("authorization".into(), "Bearer t0k".into()), ("x-tenant".into(), "acme".into())],
        tenant: None,
    };
    let outcome = run(&pipeline, &mut req);
    println!("{} -> {outcome:?}, tenant = {:?}", req.path, req.tenant);

    let mut anonymous = Request { path: "/v1/payments".into(), ..Default::default() };
    let outcome = run(&pipeline, &mut anonymous);
    println!("{} -> {outcome:?}", anonymous.path);

    println!("bad config -> {:?}", build("auth, geoip", &reg).err());

    // The generic method works on a concrete type; `pipeline[0].record(..)` would not compile.
    Auth.record(("latency_us", 412));

    // clone_box: an independent copy of the pipeline for a second listener.
    let copy: Vec<Box<dyn Middleware>> = pipeline.iter().map(|m| m.clone_box()).collect();
    println!("copied {} middleware", copy.len());

    // Upcast &dyn Middleware -> &dyn Any, then downcast to a concrete type.
    for m in &pipeline {
        let any: &dyn Any = &**m;
        if let Some(rl) = any.downcast_ref::<RateLimit>() {
            println!("rate limit configured at {}/s", rl.per_sec);
        }
    }
}
```

```text
pipeline = ["auth", "ratelimit", "tenant"]
/v1/payments -> Ok(()), tenant = Some("acme")
/v1/payments -> Err("auth: 401 missing credentials")
bad config -> Some("unknown middleware \"geoip\"")
  [auth] metric ("latency_us", 412)
copied 3 middleware
rate limit configured at 500/s
```

A note on the one place this listing needed a second try. The first draft printed `req.path`, `run(&pipeline, &mut
req)`, and `req.tenant` in a single `println!`, and rustc rejected it with E0502: the format arguments borrow `req`
shared while `run` needs it exclusively. Chapter 3.3's rule doesn't take a break for trait objects.

**Then the pipeline went multi-threaded.** The gateway runs requests on a worker pool, so the pipeline must be shared
across threads. The team's first attempt moved a `Vec<Box<dyn Middleware>>` into a thread (listing
`ch04-08-dyn-send.rs`):

```text
error[E0277]: `dyn Middleware` cannot be sent between threads safely
18 |     let worker = thread::spawn(move || pipeline.iter().all(|m| m.handle("/v1/payments")));
   |                  ------------- ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `dyn Middleware` cannot be sent between threads safely
   = help: the trait `Send` is not implemented for `dyn Middleware`
note: required because it appears within the type `Box<dyn Middleware>`
note: required because it appears within the type `Vec<Box<dyn Middleware>>`
note: required by a bound in `spawn`
```

[LANG] `Send` and `Sync` are **auto traits**: the compiler implements them for a struct when all its fields have
them (Chapter 1.3; Part XI goes deep). A trait object has *erased* its concrete type, so there are no fields to look
at, and it's `Send` only if its type says so. The fix states the requirement in the type (listing
`ch04-09-dyn-send-fixed.rs`, excerpt):

```rust,ignore
/// Auto traits are part of the object type: `dyn Middleware + Send + Sync` is a different type.
type SharedPipeline = Arc<Vec<Box<dyn Middleware + Send + Sync>>>;
```

```text
[true, true, false]
local pipeline admitted: true
```

Three workers share one pipeline through `Arc`, and the one that sent `/admin` was rejected. The same listing keeps a
`Cell`-based `LocalStats` middleware in a *single-threaded* `Vec<Box<dyn Middleware>>`. `Cell` isn't `Sync`, so
`LocalStats` can't go into the shared pipeline, and the compiler says so at the `Box::new`, not in production.

### 10. Failure scenario

**Version 2.3 of the middleware crate broke every service.** The platform team wanted typed metrics from every
middleware and added a generic method with a default body. It compiled in their crate, which never used `dyn`. Every
service that did stopped building (listing `ch04-04-dyn-incompatible.rs`):

```rust,compile_fail
use std::fmt::Debug;

struct Request {
    path: String,
    headers: Vec<(String, String)>,
}

trait Middleware {
    fn handle(&self, req: &mut Request) -> Result<(), String>;

    /// Added in v2.3 so every middleware can emit a typed metric. A GENERIC method.
    fn record<M: Debug>(&self, metric: M) {
        println!("metric: {metric:?}");
    }
}

struct Auth;

impl Middleware for Auth {
    fn handle(&self, req: &mut Request) -> Result<(), String> {
        if req.headers.iter().any(|(k, _)| k == "authorization") { Ok(()) } else { Err("401".into()) }
    }
}

fn main() {
    let pipeline: Vec<Box<dyn Middleware>> = vec![Box::new(Auth)];
    let mut req = Request { path: "/v1/payments".into(), headers: vec![] };
    for m in &pipeline {
        println!("{} -> {:?}", req.path, m.handle(&mut req));
    }
}
```

```text
error[E0038]: the trait `Middleware` is not dyn compatible
27 |     let pipeline: Vec<Box<dyn Middleware>> = vec![Box::new(Auth)];
   |                               ^^^^^^^^^^ `Middleware` is not dyn compatible
note: for a trait to be dyn compatible it needs to allow building a vtable
 9 | trait Middleware {
   |       ---------- this trait is not dyn compatible...
...
13 |     fn record<M: Debug>(&self, metric: M) {
   |        ^^^^^^ ...because method `record` has generic type parameters
   = help: consider moving `record` to another trait
```

The hotfix made it worse. Someone also wanted to copy pipelines per listener and added `trait Middleware: Clone`
(listing `ch04-05-clone-supertrait.rs`):

```text
error[E0038]: the trait `Middleware` is not dyn compatible
 7 | trait Middleware: Clone {
   |       ----------  ^^^^^ ...because it requires `Self: Sized`
   |       |
   |       this trait is not dyn compatible...
```

Both errors read directly off §4's table: a generic method has no single vtable entry, and `Clone` requires `Sized`.
The fix that shipped, in the §9 listing, kept both features:

- `record<M: Debug>` got `where Self: Sized`: out of the vtable, still available on concrete types. Later it moved to
  an extension trait (§7) so trait objects could call it too.
- `Clone` became `fn clone_box(&self) -> Box<dyn Middleware>`, written once per impl. (A macro or a helper trait with a
  blanket impl over `T: Clone` can remove the boilerplate.)
- The crate gained the `_assert_dyn_compatible` guard, and the team's semver policy now lists **"trait stops being dyn
  compatible" as a breaking change**. It's part of the trait's public API even though no signature a user wrote
  changed.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part VI).*

1. What is a trait object? Why is `dyn Trait` unsized, and what does a pointer to one contain?
2. Describe the vtable layout in current rustc. Which parts are guaranteed and which are implementation details? What
   did the IR show for a type with no drop glue?
3. Walk through `call qword ptr [rax + 24]`. Where do `rax` and `rdi` come from, and why 24?
4. Derive three dyn-compatibility rules from "a vtable is a finite table of function pointers called with an erased
   `self`."
5. What does `where Self: Sized` on a method do? When would you use an extension trait instead?
6. Why isn't `Box<dyn Trait>` automatically `Send` when every implementor happens to be `Send`?
7. Why are a trait object's generic arguments invariant while its lifetime bound is covariant?
8. Compare Java's "thin pointer, fat object" with Rust's "fat pointer, thin object." What does each design make cheap?

### 12. Exercises

- **Beginner.** Add a `GeoFence` middleware to the §9 registry, with a config argument listing allowed countries
  (`geofence=IE|DE`). Did any existing code change?
- **Intermediate.** Make the pipeline listing print `size_of_val(&**m)` and `align_of_val(&**m)` for each middleware.
  Predict the values first. What does `Auth` report, and why doesn't `Box::new(Auth)` allocate?
- **Advanced.** Write a helper trait `CloneBox` with a blanket impl `impl<T: Middleware + Clone> CloneBox for T` so
  implementors don't write `clone_box` by hand. What supertrait arrangement makes `clone_box` callable on `dyn
  Middleware`?
- **Systems.** Emit the release IR for a trait with a supertrait (`trait Middleware: Named`, with methods on both). Find
  the vtable constant and label each slot. Where are `Named`'s methods? Then upcast `&dyn Middleware` to `&dyn Named` in
  a function and find what it loads.
- **Architecture.** List every trait in a codebase you know that is used as `dyn`. For each, would adding a generic
  method, an associated const, or an `async fn` break it? Which of them have a compile-time guard?

### 13. Debugging exercise

A plugin host stores values of unknown type and inspects them with `Any` (listing `ch04-07-any-typeid-trap.rs`):

```rust
use std::any::{Any, TypeId};

fn describe(value: &dyn Any) -> String {
    if let Some(n) = value.downcast_ref::<u32>() {
        format!("u32 {n}")
    } else if let Some(s) = value.downcast_ref::<String>() {
        format!("String {s:?}")
    } else {
        "unknown type".to_string()
    }
}

fn main() {
    let items: Vec<Box<dyn Any>> = vec![Box::new(7u32), Box::new(String::from("acme")), Box::new(1.5f64)];
    for item in &items {
        println!("{}", describe(&**item));
    }

    // The trap: Box<dyn Any> is itself a 'static type, so it is also `Any`.
    let boxed: Box<dyn Any> = Box::new(7u32);
    println!("describe(&boxed)  -> {}", describe(&boxed)); // coerces the BOX to &dyn Any
    println!("describe(&*boxed) -> {}", describe(&*boxed)); // the value inside the box
    println!("boxed.type_id()    is u32? {}", boxed.type_id() == TypeId::of::<u32>());
    println!("(*boxed).type_id() is u32? {}", (*boxed).type_id() == TypeId::of::<u32>());
    println!("boxed.type_id()    is Box<dyn Any>? {}", boxed.type_id() == TypeId::of::<Box<dyn Any>>());
}
```

```text
u32 7
String "acme"
unknown type
describe(&boxed)  -> unknown type
describe(&*boxed) -> u32 7
boxed.type_id()    is u32? false
(*boxed).type_id() is u32? true
boxed.type_id()    is Box<dyn Any>? true
```

The code compiles without a warning, and a real plugin host with the `describe(&boxed)` form shipped and returned
"unknown type" for every plugin value.

1. Explain `describe(&boxed)`: what's the type of `&boxed`, which coercion turns it into `&dyn Any`, and whose
   `TypeId` ends up in the vtable?
2. Explain `boxed.type_id()` using Chapter 6.1's method-resolution steps. At which candidate type and receiver form
   does resolution stop?
3. Give two fixes, and a rule a code reviewer can apply. (Clippy has a lint for the `type_id` case. Find its name.)

### 14. Design exercise

**Partner webhook plugins.** Meridian lets partner-integration teams write *webhook transformers*: a plugin receives an
event, may rewrite it, and returns zero or more outgoing HTTP requests. Plugins are chosen per partner by config, run on
a shared worker pool, and some need per-plugin configuration and a `health()` check.

Design the plugin trait for `Arc<dyn …>` use:

- Which methods, receivers, and bounds (`Send`, `Sync`, `'static`)? Where would a generic method or `-> Self` have been
  natural, and what do you use instead?
- How are plugins constructed from config (factory registry, as in §9)? How are they cloned, if at all?
- How does an operator endpoint list each plugin's configuration without downcasting in business logic?
- The transformer may need to call a rate-limited HTTP client *asynchronously*. Note what `async fn` in the trait
  would do to dyn compatibility, and keep the question open for Part XII.
