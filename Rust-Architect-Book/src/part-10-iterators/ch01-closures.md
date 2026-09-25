# Chapter 10.1 — Closures: Fn, FnMut, FnOnce, and Capture

> **Where this sits:** Part X · Closures, Iterators, and Zero-Cost Abstractions · chapter 1 of 4
> **Prerequisites:** Chapter 2.4 (function items vs function pointers), Chapter 4.5 (closures and higher-ranked
> bounds), Chapters 6.4–6.5 (trait objects and dispatch), Chapter 7.3 (instances multiply).
> **After this chapter you can:** say exactly what a closure is in memory; predict which captures it takes and which of
> `Fn`/`FnMut`/`FnOnce` it implements; read the MIR a closure lowers to; choose between generic, `dyn`, and function
> pointer parameters by their machine code; and avoid the `move`-copies-the-counter trap that Java's effectively-final
> rule never let you write.

---

## Pass 1 · User level — *Behavior as a value, with its state made explicit*

### 1. Problem

A Java engineer passes behavior around constantly: `Comparator`s, `Predicate`s, `Runnable`s, lambdas in every stream.
Java hides the questions that matter to a systems programmer. Where does the lambda's captured state live? Can the lambda
change it? Can it be called twice? How is it called: directly, or through an interface dispatch the JIT may or may not
remove?

Rust makes every one of those questions part of the type. A function that accepts behavior has to say whether the
behavior may be called **once, repeatedly, or concurrently**, and whether the call is **static** (compiled in) or
**dynamic** (through a pointer). A closure that captures state has to own it, borrow it, or borrow it mutably, and the
borrow checker holds it to that. This chapter makes those choices precise, then shows the MIR and assembly they turn
into, because every iterator adapter in the rest of Part X is a closure wrapped in a struct.

### 2. Mental model

**A closure is an anonymous struct of its captures, plus an implementation of one or more call traits.**

```text
 let threshold = 100u64;                        struct __Closure<'a> {            // name invented; the real type
 let mut hits = 0u32;                               threshold: &'a u64,           // has no name you can write
 let mut check = |x: u64| {                         hits: &'a mut u32,
     if x > threshold { hits += 1; }            }
 };                                             impl FnMut<(u64,)> for __Closure<'_> {
                                                    fn call_mut(&mut self, (x,): (u64,)) {
                                                        if x > *self.threshold { *self.hits += 1; }
                                                    }
                                                }
```

Two questions, answered independently:

**1. How is each captured variable taken?** [LANG] For each place the body uses, the compiler picks the *least*
powerful mode that makes the body type-check: a shared borrow if the body only reads it, a unique borrow if the body
mutates it, and by value if the body moves it. `move` overrides this and takes **everything** by value (a copy for
`Copy` types, a move otherwise). Since edition 2021 the unit of capture is the **place**, not the variable: a body that
reads `cfg.limit` captures `&cfg.limit`, not `&cfg`.

**2. Which call traits does the closure implement?** That depends on what the body does to the captured state *when
called*:

| Trait | Call receiver | The body may... | Can be called | Implemented when |
|---|---|---|---|---|
| `FnOnce` | `self` (consumes the closure) | move captured values out | at most once | always (every closure) |
| `FnMut` | `&mut self` | mutate captured state | many times, one at a time | the body moves nothing out of the captures |
| `Fn` | `&self` | only read captured state | many times, even concurrently | the body also mutates nothing |

The traits nest: every `Fn` is also `FnMut`, and every `FnMut` is also `FnOnce` [LANG]. `move` does **not** decide the
trait. A `move` closure that only reads its captures is still `Fn`, and it simply owns what it reads.

**For callers of your API, the rule inverts.** Ask for the *weakest* capability you need: `FnOnce` if you call it
once, `FnMut` if you call it repeatedly from one place, `Fn` only if you need shared or concurrent calls. The weaker the
bound, the more closures your callers can pass.

### 3. Rust code

**What closures cost in bytes** (listing `ch01-01-closure-sizes.rs`, verified in debug and release):

```rust
use std::mem::{size_of, size_of_val};

struct RouteConfig {
    name: String, // 24 bytes
    limit: u64,   //  8 bytes
}

fn add_one(x: u64) -> u64 {
    x + 1
}

fn show(what: &str, bytes: usize) {
    println!("{what:<28}{bytes:>3} B");
}

fn main() {
    let cfg = RouteConfig { name: "payments".to_string(), limit: 500 };
    let threshold = 100u64;
    let mut hits = 0u32;

    // A closure is an anonymous struct holding its captures. Its size is the size of that struct.
    let no_capture = |x: u64| x + 1;
    let by_ref = |x: u64| x > threshold; // captures &threshold
    let two_refs = |x: u64| x > threshold && x < cfg.limit; // &threshold + &cfg.limit (a field!)
    let by_move = move |x: u64| x > threshold; // captures threshold itself (a u64 copy)
    let mut by_mut = |x: u64| {
        if x > threshold {
            hits += 1; // captures &mut hits (and &threshold)
        }
    };
    by_mut(150);

    show("no captures", size_of_val(&no_capture));
    show("&threshold", size_of_val(&by_ref));
    show("&threshold, &cfg.limit", size_of_val(&two_refs));
    show("move threshold (u64)", size_of_val(&by_move));
    show("&threshold, &mut hits", size_of_val(&by_mut));

    let name_len = move || cfg.name.len(); // moves ONLY cfg.name (edition 2021+ disjoint capture)
    show("move cfg.name (String)", size_of_val(&name_len));

    // Function items, function pointers, and boxed closures, for comparison.
    let item = add_one; // the fn ITEM type: a zero-sized type naming exactly one function
    let ptr: fn(u64) -> u64 = add_one; // a fn POINTER: 8 bytes, any function with this signature
    let boxed: Box<dyn Fn(u64) -> u64> = Box::new(move |x| x + threshold);
    show("fn item `add_one`", size_of_val(&item));
    show("fn pointer", size_of_val(&ptr));
    show("Box<dyn Fn> (data + vtable)", size_of_val(&boxed));
    println!("hits = {hits}; cfg.limit still usable: {}", cfg.limit);
    assert_eq!(size_of::<fn(u64) -> u64>(), 8);
    let _ = (no_capture(1), by_ref(1), two_refs(1), by_move(1), name_len(), item(1), ptr(1), boxed(1));
}
```

```text
no captures                   0 B
&threshold                    8 B
&threshold, &cfg.limit       16 B
move threshold (u64)          8 B
&threshold, &mut hits        16 B
move cfg.name (String)       24 B
fn item `add_one`             0 B
fn pointer                    8 B
Box<dyn Fn> (data + vtable)  16 B
hits = 1; cfg.limit still usable: 500
```

Every number is the struct from §2. A closure with no captures is a zero-sized type, exactly like the function item
`add_one` (Chapter 2.4). A closure that captures two references is two pointers. `move || cfg.name.len()` is 24 bytes
because it holds the moved `String`, and `cfg.limit` is still usable afterwards because only the field was moved.

**Which trait the compiler infers** (listing `ch01-02-fn-traits.rs`):

```rust
// Which of Fn / FnMut / FnOnce does each closure implement? The compiler decides from the BODY:
// what the body does with each capture, not how the capture was taken (`move` or not).

fn call_fn(f: impl Fn() -> usize) -> usize {
    f() + f() // may call any number of times, through &self
}
fn call_fn_mut(mut f: impl FnMut() -> usize) -> usize {
    f() + f() // may call any number of times, through &mut self
}
fn call_fn_once(f: impl FnOnce() -> usize) -> usize {
    f() // may call at most once: calling consumes self
}

fn main() {
    let routes = vec!["/pay".to_string(), "/refund".to_string()];

    // Only READS its captures -> Fn (and therefore also FnMut and FnOnce).
    let count = || routes.len();
    println!("Fn     via call_fn:      {}", call_fn(count));
    println!("Fn     via call_fn_mut:  {}", call_fn_mut(count));
    println!("Fn     via call_fn_once: {}", call_fn_once(count));

    // MUTATES a capture -> FnMut (and FnOnce), but not Fn.
    let mut calls = 0;
    let bump = || {
        calls += 1;
        calls
    };
    println!("FnMut  via call_fn_mut:  {}", call_fn_mut(bump));

    // MOVES a capture out of itself -> FnOnce only.
    let hand_off = move || {
        let owned: Vec<String> = routes; // moves the captured Vec out of the closure
        owned.len()
    };
    println!("FnOnce via call_fn_once: {}", call_fn_once(hand_off));

    // `move` changes HOW captures are taken, not which trait is implemented:
    let limit = 3usize;
    let reads_moved = move || limit * 2; // owns its copy of `limit`, only reads it -> still Fn
    println!("move + read-only is Fn:  {}", call_fn(reads_moved));
    println!("calls after bump: {calls}");
}
```

```text
Fn     via call_fn:      4
Fn     via call_fn_mut:  4
Fn     via call_fn_once: 2
FnMut  via call_fn_mut:  3
FnOnce via call_fn_once: 2
move + read-only is Fn:  12
calls after bump: 2
```

One detail worth noticing: `count` is passed **by value** three times. That works because a closure is `Copy` when all
its captures are `Copy`, and `&Vec<String>` is. `bump` holds a `&mut`, so it isn't `Copy`, and it was moved into
`call_fn_mut`.

**Calling an `FnOnce` twice** (listing `ch01-03-fnonce-twice.rs`, verified):

```rust,compile_fail
fn main() {
    let batch = vec![10_u64, 20, 30];
    let flush = move || {
        let owned = batch; // moves the captured Vec out: this closure is FnOnce only
        owned.iter().sum::<u64>()
    };
    println!("first flush: {}", flush());
    println!("second flush: {}", flush()); // a second call would use a moved-out Vec
}
```

```text
error[E0382]: use of moved value: `flush`
 --> src/main.rs:9:34
  |
8 |     println!("first flush: {}", flush());
  |                                 ------- `flush` moved due to this call
9 |     println!("second flush: {}", flush()); // a second call would use a moved-out Vec
  |                                  ^^^^^ value used here after move
  |
note: closure cannot be invoked more than once because it moves the variable `batch` out of its environment
```

It's an ordinary use-after-move, because `FnOnce::call_once` takes `self`. The note names the capture responsible.

**The same mistake, two different errors.** Passing a mutating closure where `Fn` is required gives a different error
depending on *where the closure is written* (listings `ch01-04-fnmut-where-fn.rs` and `ch01-13-fnmut-bound-first.rs`):

```rust,compile_fail
fn run_twice<F: Fn()>(f: F) {
    f();
    f();
}

fn main() {
    let mut retries = 0;
    run_twice(|| retries += 1); // the Fn bound drives inference: this closure is checked AS an Fn
    println!("{retries}");
}
```

```text
error[E0594]: cannot assign to `retries`, as it is a captured variable in a `Fn` closure
 --> src/main.rs:9:18
  |
2 | fn run_twice<F: Fn()>(f: F) {
  |                          - change this to accept `FnMut` instead of `Fn`
...
9 |     run_twice(|| retries += 1); // the Fn bound drives inference: this closure is checked AS an Fn
  |     --------- -- ^^^^^^^^^^^^ cannot assign
  |     |         |
  |     |         in this closure
  |     expects `Fn` instead of `FnMut`
```

```rust,compile_fail
fn run_twice<F: Fn()>(f: F) {
    f();
    f();
}

fn main() {
    let mut retries = 0;
    let bump = || retries += 1; // no expected type here: inferred as FnMut from the body
    run_twice(bump); // ...and FnMut doesn't satisfy an Fn bound
    println!("{retries}");
}
```

```text
error[E0525]: expected a closure that implements the `Fn` trait, but this closure only implements `FnMut`
  --> src/main.rs:9:16
   |
 9 |     let bump = || retries += 1; // no expected type here: inferred as FnMut from the body
   |                ^^ ------- closure is `FnMut` because it mutates the variable `retries` here
   |                |
   |                this closure implements `FnMut`, not `Fn`
10 |     run_twice(bump); // ...and FnMut doesn't satisfy an Fn bound
   |     --------- ---- the requirement to implement `Fn` derives from here
```

When a closure is written directly in the argument position, the bound is the **expected type**, and the compiler
type-checks the body *as an `Fn`*, so the assignment itself is the error (E0594). Written first and passed later, the
closure's kind is inferred from its body (FnMut), and the *mismatch* is the error (E0525). Chapter 4.5 used the same
mechanism to get higher-ranked signatures for closures that return references. Both messages suggest the right fix:
the API should accept `FnMut`.

**Disjoint capture, and the edition that introduced it** (listing `ch01-06-disjoint-capture.rs`, verified under
editions 2024 and 2018):

```rust
// Edition 2021+ closures capture the PLACES they use (cfg.name), not whole variables (cfg).
struct Upstream {
    name: String,
    timeout_ms: u64,
}

fn main() {
    let cfg = Upstream { name: "processor-a".to_string(), timeout_ms: 250 };
    let log_name = move || println!("upstream = {}", cfg.name); // moves only cfg.name (2021+)
    log_name();
    println!("timeout still readable: {} ms", cfg.timeout_ms); // 2018: `cfg` was moved whole
}
```

```text
upstream = processor-a
timeout still readable: 250 ms
```

Under edition 2018 the same file fails with `error[E0382]: borrow of moved value: cfg ... value moved into closure
here`. [VERSION] RFC 2229 ("disjoint closure captures") shipped with edition 2021. It changes *which* values a closure
owns, and therefore **when they're dropped**, which is why `cargo fix --edition` inserts `let _ = &cfg;` into closures
where the drop order would have changed. Chapter 5.3 showed the other consequence: a closure that captures only a
`u64` field doesn't inherit the containing struct's `!Send`.

**Returning closures** (listing `ch01-08-return-closures.rs`):

```rust
// Returning closures: one concrete type -> `impl Fn`; a choice among several -> Box<dyn Fn> (or an enum).
fn fee_calculator(bps: u64) -> impl Fn(u64) -> u64 {
    move |amount_cents| amount_cents * bps / 10_000
}

fn rounding_rule(kind: &str) -> Box<dyn Fn(u64) -> u64> {
    match kind {
        "floor10" => Box::new(|c| c / 10 * 10),
        "ceil10" => Box::new(|c| c.div_ceil(10) * 10),
        _ => Box::new(|c| c),
    }
}

fn main() {
    let fee = fee_calculator(290); // 2.90%
    let round = rounding_rule("ceil10");
    for amount in [1_000, 12_345, 99_999] {
        println!("amount {amount:>6} -> fee {:>4} -> rounded {:>4}", fee(amount), round(fee(amount)));
    }
    println!("size_of_val(fee) = {} B, size_of_val(round) = {} B",
        std::mem::size_of_val(&fee), std::mem::size_of_val(&round));
}
```

```text
amount   1000 -> fee   29 -> rounded   30
amount  12345 -> fee  358 -> rounded  360
amount  99999 -> fee 2899 -> rounded 2900
size_of_val(fee) = 8 B, size_of_val(round) = 16 B
```

`impl Fn` returns **one** concrete, unnameable type (8 bytes: the captured `bps`). Try to return one of two capturing
closures and the compiler says why that can't work (listing `ch01-09-impl-fn-two-closures.rs`):

```text
error[E0308]: `if` and `else` have incompatible types
  = note: expected closure `{closure@src/main.rs:4:9: 4:22}`
             found closure `{closure@src/main.rs:6:9: 6:22}`
  = note: no two closures, even if identical, have the same type
  = help: consider boxing your closure and/or using it as a trait object
```

> **What actually happens?** My first draft of that listing used two closures that captured *nothing*, and it
> compiled. [LANG] Non-capturing closures coerce to function pointers, and an `if`/`else` whose branches have different
> closure types looks for a common type: `fn(u64) -> u64`. The function returned `impl Fn` backed by an 8-byte function
> pointer (listing `ch01-14-noncapturing-coerce.rs` prints `12350 (8 B: a fn pointer)`). It's correct code, but every
> call now goes through a pointer. Capturing closures have no such escape hatch (listing `ch01-10-capturing-to-fnptr.rs`:
> *"closures can only be coerced to `fn` types if they do not capture any variables"*).

---

## Pass 2 · Systems level — *What a closure lowers to, and what each calling style costs*

### 4. Under the hood

**The MIR of a closure** (listing `ch01-05-closure-mir.rs`; `tools/emit.ps1 -Target mir -Mode debug`, rustc 1.98.1):

```rust,ignore
#[inline(never)]
pub fn count_over(values: &[u64], threshold: u64) -> usize {
    let mut seen = 0usize;
    let mut check = |v: u64| {
        if v > threshold {
            seen += 1;
        }
    };
    for &v in values {
        check(v);
    }
    seen
}
```

Construction, in the enclosing function (trimmed):

```text
    bb0: {
        _3 = const 0_usize;                                   // seen
        _5 = &_2;                                             // &threshold
        _6 = &mut _3;                                         // &mut seen
        _4 = {closure@src/lib.rs:7:21: 7:29} { threshold: move _5, seen: move _6 };
        ...
    bb5: {
        ...
        _14 = &mut _4;
        _15 = (copy _12,);
        _13 = <{closure@src/lib.rs:7:21: 7:29} as FnMut<(u64,)>>::call_mut(move _14, move _15) -> [return: bb2, ...];
```

And the body, compiled as its own function:

```text
fn count_over::{closure#0}(_1: &mut {closure@src/lib.rs:7:21: 7:29}, _2: u64) -> () {
    debug v => _2;
    debug threshold => (*((*_1).0: &u64));
    debug seen => (*((*_1).1: &mut usize));
    ...
    bb0: {
        _6 = no_retag copy ((*_1).0: &u64);
        _4 = copy (*_6);
        _3 = Gt(copy _2, move _4);
        switchInt(move _3) -> [0: bb3, otherwise: bb1];
    }
```

That's §2's diagram, produced by the compiler. The closure value `_4` is an aggregate with two fields, `threshold:
&u64` and `seen: &mut usize`. The body is a function whose first parameter is `&mut` that aggregate (an `FnMut`
receiver), and every use of a captured variable is a field projection through it: `(*((*_1).1: &mut usize))` is "the
`usize` that field 1 points to." The call goes through `FnMut::call_mut` with the arguments packed into a **tuple**
`(u64,)`.

The tuple is a real detail. [RUSTC] [VERSION] The `Fn*` traits are declared with the unstable `"rust-call"` ABI, whose
last parameter is a tuple of arguments. That's also why you can't write `impl Fn for MyType` on stable Rust: the
features behind it (`unboxed_closures`, `fn_traits`) aren't stable. Closures are the only stable way to implement them.

**Capture analysis happens during type checking** [RUSTC]. rustc's "upvar analysis" walks the closure body after type
inference, records how each captured place is used, and picks the capture mode. Then the closure's type gets its
fields and its call traits. It runs before borrow checking, so the borrows a closure holds are ordinary loans that the
borrow checker sees at the construction site. `check` holds `&mut seen` from line 7 until its last call, and any use of
`seen` in between would be E0499/E0502 (Chapter 4.1).

**Signature inference, and its sharpest edge.** A closure's parameter and return types are inferred too, and §3 showed
the rule that drives them: if the closure is written where an `Fn*` bound is expected, the bound supplies the
signature. That rule matters most for references. Chapter 4.5 showed a standalone closure `|s: &str| -> &str { .. }`
failing with *"lifetime may not live long enough"*: without an expected signature, the compiler gives the parameter and
the return value two unrelated lifetimes, because closures don't get function-style elision. Chapter 4.5 fixed it with
a function item, or by passing the closure straight to a function with a `for<'a>` bound. A third fix lets you keep the
closure in a variable (listing `ch01-17-closure-sig-helper.rs`):

```rust
// A third fix for Chapter 4.5's "closure returning a reference" error: an identity helper whose bound supplies
// the higher-ranked signature, so the closure can be stored in a variable and reused.
fn returns_borrow<F: Fn(&str) -> &str>(f: F) -> F {
    f // `Fn(&str) -> &str` in a bound means `for<'a> Fn(&'a str) -> &'a str` (elision in Fn sugar)
}

fn main() {
    let first_word = returns_borrow(|s| s.split(' ').next().unwrap_or(""));
    let methods: Vec<&str> = ["GET /health", "POST /pay"].iter().map(|l| first_word(l)).collect();
    println!("{methods:?}");
}
```

```text
["GET", "POST"]
```

The helper does nothing at run time (it returns its argument, and it's inlined away). Its only job is to be the
"expected type" at the point where the closure is written. [VERSION] Stable Rust still has no syntax for writing
`for<'a>` on a closure directly, so this identity-function idiom is common in libraries that store borrowing closures.

**One closure type per enclosing instance** (listing `ch01-12-closure-per-instance.rs`):

```rust
// A closure written inside a generic function is generic too: one closure TYPE per instance
// of the enclosing function (Chapter 7.3 relied on this).
use std::any::type_name_of_val;

fn describe<T: std::fmt::Debug>(items: &[T]) -> Vec<String> {
    let fmt_one = |item: &T| format!("{item:?}");
    println!("closure type: {}", type_name_of_val(&fmt_one));
    items.iter().map(fmt_one).collect()
}

fn main() {
    println!("{:?}", describe(&[1u8, 2]));
    println!("{:?}", describe(&["a", "b"]));
    println!("{:?}", describe(&[1.5f64]));
}
```

```text
closure type: playground::describe<u8>::{{closure}}
["1", "2"]
closure type: playground::describe<&str>::{{closure}}
["\"a\"", "\"b\""]
closure type: playground::describe<f64>::{{closure}}
["1.5"]
```

The closure inherits every generic parameter of the function that contains it. Three instances of `describe` mean
three closure types, three `Map<Iter<T>, {closure}>` types, and three copies of everything they instantiate. This is
the mechanism behind Chapter 7.3's advice to keep closures out of large generic functions when code size matters.

### 5. Memory

A closure lives **wherever you put it**, like any struct:

```text
 let check = |v| ...            on the stack, in count_over's frame: 16 bytes (two pointers into that same frame)
 xs.iter().filter(check)        moved INTO the Filter struct: the adapter's size grows by the closure's size (10.3)
 Box::new(check) as Box<dyn Fn> the captures move to the heap; the Box is a 16-byte fat pointer (data, vtable)
                                 (a zero-sized closure boxed as dyn Fn allocates nothing: Box of a ZST doesn't allocate)
 thread::spawn(move || ...)     moved to the new thread's first stack frame; must own everything ('static, Chapter 4.3)
```

A closure that *borrows* holds pointers into its creator's stack frame. That's the whole reason borrowed captures can't
outlive the frame. It's why `thread::spawn` demands `'static` and `move` (E0373, Chapter 4.3), and why the debugging
exercise's loop fails with E0597.

The **vtable** behind `&dyn Fn(u64) -> u64` is visible in the release assembly of listing
`ch01-11-closure-dispatch-asm.rs` (rustc 1.98.1, labels simplified):

```text
.Lanon...0:                                         ; the vtable for fee_dyn's closure as dyn Fn(u64) -> u64
	.asciz	"\000\000\000\000\000\000\000\000\b\000\000\000\000\000\000\000\b\000\000\000\000\000\000"
	                                                ; [0]  drop_in_place: null (a &u64 capture needs no drop)
	                                                ; [8]  size  = 8
	                                                ; [16] align = 8
	.quad	<fee_dyn::{closure#0} as FnOnce<(u64,)>>::call_once::{shim:vtable#0}   ; [24] FnOnce::call_once
	.quad	playground::fee_dyn::{closure#0}                                        ; [32] FnMut::call_mut
	.quad	playground::fee_dyn::{closure#0}                                        ; [40] Fn::call
```

Six words: the three that every vtable starts with (drop, size, align; Chapter 6.4), then one slot per method of `Fn`
*and its supertraits*. `call_mut` and `call` point to the same code, because for an `Fn` closure they do the same thing.
[RUSTC] The vtable layout is an implementation detail, not a guarantee.

### 6. CPU / OS

The same listing passes one closure three ways. Here's what each calling style compiles to (release, trimmed, labels
simplified).

**Generic `F: Fn(u64) -> u64`**. The instance `apply_generic::<fee_generic::{closure#0}>` has the closure inlined:

```text
playground::apply_generic::<playground::fee_generic::{closure#0}>:
	...
	movabs	r9, 3777893186295716171          ; magic constant: /10_000 becomes multiply + shift
.LBB0_5:                                     ; unrolled by 2
	mov	rax, qword ptr [rsi + 8*r10]         ; x
	imul	rax, rdi                         ; x * bps
	mul	r9                                   ; ... / 10_000 (high half of the product)
	mov	r8, rdx
	shr	r8, 11
	add	r8, rcx                              ; running sum
	mov	rax, qword ptr [rsi + 8*r10 + 8]     ; next x
	imul	rax, rdi
	mul	r9
	...
	add	r10, 2
	cmp	rbx, r10
	jne	.LBB0_5
```

**`&dyn Fn(u64) -> u64`**. One indirect call per element:

```text
playground::apply_dyn:
	push rbp / r15 / r14 / r13 / r12 / rbx      ; six callee-saved registers: the loop must survive calls
	...
	mov	r13, qword ptr [rsi + 40]            ; load Fn::call from vtable slot 5, once
.LBB6_3:
	mov	rsi, qword ptr [r14 + 8*rbp]         ; x
	mov	rdi, r12                             ; the closure's data pointer
	call	r13                              ; indirect call, per element
	add	r15, rax
	inc	rbp
	cmp	rbx, rbp
	jne	.LBB6_3
```

**`fn(u64) -> u64`**. The same shape, `call r12` per element. And `double_ptr` passes its non-capturing closure's
`call_once` body as the pointer:

```text
playground::double_ptr:
	lea	rdi, [rip + <playground::double_ptr::{closure#0} as core::ops::function::FnOnce<(u64,)>>::call_once]
	jmp	qword ptr [rip + playground::apply_ptr@GOTPCREL]
```

The difference isn't the `call` instruction. A correctly predicted indirect call costs a few cycles on a modern x86-64
core [CPU], and a call site that always targets the same function is predicted well. The difference is **everything
the call prevents**. With the closure inlined, LLVM knew the operation, replaced the division with a multiply-shift,
unrolled the loop, and kept the sum in registers. Behind a pointer, the loop body is opaque: no unrolling, no
vectorization, no constant folding, and registers saved and restored around every call. Chapter 6.5 measured the
consequence for a similar loop (static per-type 0.80–0.82 ns per element, `dyn` over mixed types 3.26–3.28 ns), and
Chapter 10.3 measures it for iterators.

---

## Pass 3 · Architect level — *Choosing how behavior crosses an API*

### 7. Trade-offs

| Parameter or return type | Dispatch | Inlining | Allocation | Holds different closures in one collection? | Code size / compile time |
|---|---|---|---|---|---|
| `f: F` where `F: Fn(..)` (or `impl Fn`) | Static | Yes | None | No (one type per instance) | One copy per closure type (Chapter 7.3) |
| `f: &dyn Fn(..)` | Indirect call | No (unless devirtualized) | None | Yes | One copy |
| `Box<dyn Fn(..) + Send + Sync>` | Indirect call | No | One per boxed closure that captures | Yes | One copy |
| `f: fn(..)` (function pointer) | Indirect call | No (unless constant-propagated) | None | Yes, but only non-capturing | One copy |
| `enum Behavior { A(..), B(..) }` + `match` | Static per arm | Yes | None | Yes, closed set | One copy, grows per variant |
| return `impl Fn(..)` | Static | Yes | None | n/a (exactly one type) | Unnameable type: can't store it in a struct field without generics |

Three design rules follow:

1. **Take `impl FnMut` (generic) by default** for callbacks invoked in a loop. It's the fastest and the most permissive
   for callers. Switch to `&mut dyn FnMut` when the function is large and called with many closure types (code size),
   or when it must be non-generic (a trait method that must stay `dyn`-compatible, Chapter 6.4).
2. **Store `Box<dyn Fn + Send + Sync>`** when behavior is chosen at run time (configuration, plugins, registries). Add
   `Send + Sync` at the type, not later: every stored closure must then be shareable across threads, and the compiler
   checks it at the point where each closure is boxed.
3. **Name your weakest need.** `FnOnce` for "run this once" (thread bodies, `Option::map_or_else`, completion
   handlers); `FnMut` for sequential repeated calls (iterator adapters, visitors); `Fn` only for concurrent or shared
   calls (middleware shared across worker threads, `Arc<dyn Fn>`).

The same three-level hierarchy exists for asynchronous callbacks. Async closures (`async |x| { .. }`) and the
`AsyncFn` / `AsyncFnMut` / `AsyncFnOnce` traits have been stable since Rust 1.85 [VERSION]. They exist because
`F: Fn() -> Fut` can't express a future that borrows from the closure's own captures. Part XII covers them, after it
explains what a future is.

> **Why not always `Box<dyn Fn>`, like Java?** Because it throws away the thing Rust's closures were designed for.
> Before Rust 1.0, closures *were* boxed and dynamically called. RFC 114 ("unboxed closures", 2014) replaced them with
> the struct-plus-trait design in §2, precisely so that iterator adapters could be inlined. Boxing is a tool for
> heterogeneity and type erasure, not a default.

### 8. Java comparison

| | Java lambda | Rust closure |
|---|---|---|
| Representation | An object of a hidden class implementing a functional interface, spun at link time by `invokedynamic` + `LambdaMetafactory` (Java 8, JSR 292) | An anonymous struct of captures + compiler-generated `Fn*` impls; no class, no header, no allocation by itself |
| Captured locals | Must be *effectively final* (JLS 15.27.2); their **values** are copied into the lambda object | Captured by shared borrow, unique borrow, or move, per place, inferred; `move` forces by-value |
| Mutating captured state | Impossible for locals (people use `int[] counter = {0}` or `AtomicInteger`) | `FnMut`, checked by the borrow checker |
| Consuming captured state | No concept | `FnOnce`, checked at compile time |
| Allocation | Non-capturing: OpenJDK reuses one instance per call site. Capturing: a new object per evaluation, unless escape analysis scalar-replaces it (JLS 15.27.4 permits either) | None, unless you box it |
| Call | `invokeinterface` on the functional interface; the JIT inlines it if the call site's profile is monomorphic | Static call inlined at compile time (generic), or an indirect call (`dyn`, `fn` pointer) |
| Type | The target interface (`Predicate<T>`, `Function<A,B>`, ...) | A unique unnameable type; functions accept it via `impl Fn(..)` / generics / `dyn Fn(..)` |

The effectively-final rule is worth understanding, not just memorizing. Because a Java lambda copies captured values, a
lambda that could assign to a local would be assigning to *its copy*, and the enclosing method would never see the
change. The language forbids the confusing case outright. Rust permits mutation through a *borrowed* capture, and
the borrow checker makes it safe. But Rust also lets you copy with `move`, and then it will let you mutate the copy.
§10 is what happens when you do that by accident.

> **Analogy limit.** "A Rust closure is a Java lambda" holds for syntax and for the common case of passing behavior to
> a library. It fails on three points. **Cost**: a Java lambda is always an object reached through an interface; a Rust
> closure is usually neither. **State**: Java's captured locals are frozen copies; Rust's captures can be live borrows
> that the closure mutates. **Identity**: every Rust closure has its own type, so two identical-looking closures can't
> share a variable or a return type without boxing or a function-pointer coercion.

### 9. Production scenario

**Meridian's gateway log sampler.** The gateway (400K req/s at peak; Chapter 1.2) writes one access-log record per
request, but only a sample is shipped to the analytics pipeline. The on-call team controls the sample with rules in
configuration, such as `status>=500 || path^=/payments && latency>250`. A rule change must not need a deploy.

There are three ways to evaluate a rule, measured in listing `ch01-15-compiled-filter.rs` on 200,000 synthetic records
(release, best of 7, one Playground run, noisy):

```text
AST interpreter           11.45 ns/record   kept 40000
compiled closures          6.28 ns/record   kept 40000
hand-written               0.89 ns/record   kept 40000
```

The **interpreter** parses the rule into an `enum Expr` tree once, then walks the tree with a `match` for every record.
**Closure compilation** walks the tree *once* and produces a tree of boxed closures, each capturing its operands and its
children (abridged from the listing):

```rust,ignore
type Pred = Box<dyn Fn(&Record) -> bool + Send + Sync>;

/// Closure compilation: walk the AST ONCE, producing nested closures that capture their operands.
fn compile(e: Expr) -> Pred {
    match e {
        Expr::StatusGe(s) => Box::new(move |r| r.status >= s),
        Expr::LatencyGt(l) => Box::new(move |r| r.latency_ms > l),
        Expr::PathPrefix(p) => Box::new(move |r| r.path.starts_with(p.as_str())),
        Expr::And(a, b) => {
            let (a, b) = (compile(*a), compile(*b));
            Box::new(move |r| a(r) && b(r))
        }
        Expr::Or(a, b) => {
            let (a, b) = (compile(*a), compile(*b));
            Box::new(move |r| a(r) || b(r))
        }
    }
}
```

The per-record `match` on the node type is gone: each closure already *is* its node's behavior. That's the classic
"closure compilation" technique for interpreters, and here it was 1.8× faster than the tree walk. **Hand-written** code
(what a code generator or a fixed rule would produce) is another 7× faster: no indirect calls at all, and LLVM can
reorder the cheap integer comparison ahead of the string prefix test.

The team's decision was about volume, not about the fastest number. At 400K req/s, even the interpreter costs 400,000
× 11.45 ns ≈ 4.6 ms of CPU per second, under 0.5% of one core across the whole fleet's traffic. Rules stayed
configurable, and closure compilation was chosen because it was also the simplest code: `compile` is 20 lines, and the
result is a `Pred` stored in an `ArcSwap` (Part XI) so that a rule change is one atomic pointer swap. The `+ Send +
Sync` on `Pred` is what made that sharing legal. The analytics pipeline, which re-evaluates rules over billions of
archived records, is where the 7× matters, and it compiles its fixed rules into Rust code.

### 10. Failure scenario

**The retry counter that never counted.** The gateway's upstream client wraps each call in a retry helper that takes an
`FnMut` (listing `ch01-07-move-copies-counter.rs`, abridged; the full listing is verified):

```rust,ignore
fn main() {
    let mut attempts = 0u32; // meant to be reported in the access log
    let mut failures_left = 2; // the upstream fails twice, then succeeds
    let result = with_retries(
        move || {
            attempts += 1; // increments the CLOSURE's copy of `attempts`
            if failures_left > 0 {
                failures_left -= 1;
                Err("upstream timeout")
            } else {
                Ok(attempts)
            }
        },
        5,
    );
    println!("result = {result:?}, attempts logged = {attempts}");
}
```

```text
result = Ok(3), attempts logged = 0
```

The closure was originally non-`move`, and `attempts` was captured by `&mut`. During a refactor the call moved into a
spawned task. The compiler then asked for `move` (E0373), the developer added it, and everything compiled. `u32` is
`Copy`, so `move` put a **copy** of `attempts` inside the closure. The retries still happened (the closure's copy
counted 1, 2, 3 correctly, which is what `Ok(3)` shows), but the access log reported the outer variable: 0, on every
request. There was no warning, because the closure *reads* its copy (in `Ok(attempts)`), so nothing looks unused.

The consequence arrived weeks later. Meridian's retry-budget alert (Chapter 8.4) watched the `attempts` field, and
during a processor brownout it stayed flat while the gateway tripled its upstream traffic.

The fixes, from best to worst:

1. **Let the helper own the counting.** `with_retries` returns `(Result<T, E>, attempts)`, so no closure needs to
   report anything. The data flows out through the return value.
2. **Share it explicitly.** `Arc<AtomicU32>` (for a spawned task) or `Cell<u32>` (single thread) captured by
   reference or `Arc` clone. `move` then moves the `Arc`, not the counter.
3. **Never mutate a `move`-captured `Copy` value and expect the outside to see it.** A lint can't catch every case, but
   when the copy is written and never read, rustc says so. Part X's review capstone contains exactly that warning:
   `value captured by ... is never read` with `help: did you mean to capture by reference instead?`

The Java-shaped lesson is the useful one. Java's effectively-final rule forbids this bug by forbidding the pattern.
Rust allows the pattern because borrowed captures make it safe, and `move` quietly turns a borrow into a copy.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part X).*

1. What is a closure in Rust, concretely? What determines its size?
2. How does the compiler decide how each variable is captured? What does `move` change, and what doesn't it change?
3. Explain `Fn`, `FnMut`, and `FnOnce` in terms of their receivers. Why does every `Fn` also implement `FnOnce`?
4. When you write a function that takes a callback, how do you choose which of the three traits to require?
5. Why can two closures with identical code not be returned from the two branches of an `if`? What are your options?
6. What changed about closure capture in edition 2021, and why can that change drop order?
7. Compare the machine code of a generic `F: Fn` parameter, a `&dyn Fn`, and a `fn` pointer. Where does the cost of
   dynamic dispatch actually come from?
8. How do Java lambdas capture variables, and why does Java require captured locals to be effectively final?
9. A teammate says "closures in Rust are zero-cost." Under what conditions is that true, and when isn't it?
10. How would you design a configuration-driven rule engine so that rules can change at run time but evaluation stays
    fast?

### 12. Exercises

- **Beginner.** Predict `size_of_val` for closures capturing: nothing; a `&str`; a moved `&str`; a moved `Vec<u8>`; a
  `&mut [u8; 1024]`; a moved `[u8; 1024]`. Verify on the Playground.
- **Intermediate.** Write `fn compose<A, B, C>(f: impl Fn(A) -> B, g: impl Fn(B) -> C) -> impl Fn(A) -> C`. Then
  write a version that works with `FnMut`. What must the return type be, and why?
- **Advanced.** Write a `memoize` helper that takes `impl Fn(u64) -> u64` and returns a closure that caches results in
  a `HashMap`. Which trait can the returned closure implement? Now make it `Fn` anyway. What did you have to use, and
  what does that cost?
- **Systems.** Emit the release assembly of `apply_generic` instantiated with two *different* closures. Count the
  instances. Then change `apply_generic` to call a non-generic inner function that takes `&dyn Fn`. What happens to
  code size, and what happens to the inner loop?
- **Architecture.** Meridian's in-process event bus (Chapter 3.6) will become multi-threaded. List every property its
  subscriber type (`Box<dyn Fn(&Event) + Send + Sync>`, `Box<dyn FnMut(&Event) + Send>`, or a channel per subscriber)
  would give or take away: ordering, back-pressure, reentrancy, panics in handlers, unsubscribe.

### 13. Debugging exercise

```rust,ignore
fn main() {
    let mut handlers: Vec<Box<dyn Fn() -> u32>> = Vec::new();
    for shard in 0..3 {
        handlers.push(Box::new(|| shard * 10)); // borrows the loop variable, which dies each iteration
    }
    for h in &handlers {
        println!("{}", h());
    }
}
```

1. Predict the error code and the three places the error points to. (It's verified in listing
   `ch01-16-capture-loop-var.rs`.)
2. Why is the closure borrowing `shard` rather than copying it, when `shard` is a `u32`?
3. Fix it. Then change `shard` to a `String` (`format!("shard-{i}")`). What does each closure now own, and what does
   the fix cost per closure?

### 14. Design exercise

**A retry helper for payments-core.** Chapter 8.4 established Meridian's rules: retry only transient and
ambiguous-if-idempotent errors, full-jitter backoff, a deadline budget, and one idempotency key per logical operation.
Design `retry` as a library function. Decide:

- The operation's type: `FnMut() -> Result<T, E>`, `FnMut(Attempt) -> Result<T, E>` (passing the attempt number and
  remaining budget in), or `Fn` (and why you'd need it). What about async (Part XII)?
- How callers get the attempt count and the classification of the final error, without capturing a counter.
- What happens if the operation panics on attempt 2.
- Whether the backoff policy is a generic parameter, a `Box<dyn BackoffPolicy>`, or plain data, given that 40 services
  will call this helper.

Write the signature and justify each choice in one sentence.
