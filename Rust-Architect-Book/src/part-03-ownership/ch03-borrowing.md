# Chapter 3.3 — Borrowing: Shared and Mutable References

> **Where this sits:** Part III · Ownership · chapter 3 of 6
> **Prerequisites:** Chapters 3.1–3.2.
> **After this chapter you can:** use `&T` and `&mut T` fluently; state the borrowing rules as *aliasing XOR mutation*;
> read the common borrow errors (E0502, E0499, E0505, E0506) as ownership arguments; show in real assembly what the
> rules let the optimizer do; and design APIs whose parameter types say exactly what they do with your data.

---

## Pass 1 · User level — *Temporary access without ownership*

### 1. Problem

If moves were the only way to hand a value to a function, every call would look like
`let (v, result) = compute(v);`, threading ownership in and out. Most functions only need to *look at* or *modify*
something for a while, then give it back. That's **borrowing**: temporary access through a **reference**, with rules
that guarantee the reference can't outlive the owner and can't be used to break it.

Part IV takes the borrow *checker* apart: non-lexical lifetimes, annotations, variance, and reading every error as a
proof. This chapter establishes what borrowing *is*, why its rules are exactly these two, and what they buy you at the
machine level.

### 2. Mental model

Two kinds of reference:

| | `&T`: shared reference | `&mut T`: mutable (exclusive) reference |
|---|---|---|
| How many at once | Any number | **Exactly one**, and no `&T` alongside it |
| May read | Yes | Yes |
| May write | No (except through interior mutability, Chapter 3.6) | Yes |
| While it's live, the *owner* may... | only read | **nothing**: the owner is fully locked out |
| `Copy`? | Yes | No (a copy would mean two exclusive references) |

**The two rules** [LANG]:

1. **Aliasing XOR mutation.** At any point, a value may have *either* one mutable reference *or* any number of shared
   references, never both.
2. **References never outlive their referent.** A reference is always valid: non-null, aligned, pointing to a live,
   initialized value of its type.

Think of a borrow as a **loan with a deadline**. A shared loan lets many people read the document at once, and nobody,
not even the owner, may edit it until every reader has returned it. An exclusive loan hands the document to one person,
who may edit it, and nobody else may even look until it comes back. The deadline, the region where the loan is live, is
what Part IV calls a **lifetime**. [VERSION] Since the 2018 edition, a loan ends at its **last use**, not at the end of
the enclosing block (non-lexical lifetimes, NLL).

```text
 time ───────────────────────────────────────────────────────────────────────►
 owner `v`   ██████████ reads ██████████ │ locked │ ████ reads & writes ██████
 &v (r1)          ├──────────────┤  (last use)
 &v (r2)             ├────────────────┤
 &mut v (m)                                ├──────┤  exclusive: no other access in this window
```

### 3. Rust code

Borrowing in everyday use (verified):

```rust
fn average(xs: &[u32]) -> Option<f64> {
    // a shared borrow: read-only access
    if xs.is_empty() {
        return None;
    }
    Some(xs.iter().map(|&x| x as f64).sum::<f64>() / xs.len() as f64)
}

fn normalize(xs: &mut Vec<u32>) {
    // an exclusive borrow: may mutate
    xs.sort_unstable();
    xs.dedup();
}

fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    // the returned reference borrows from the inputs (Part IV explains 'a)
    if a.len() >= b.len() { a } else { b }
}

fn main() {
    let mut latencies = vec![120, 45, 45, 300, 80];

    let avg = average(&latencies); // readers...
    let r1 = &latencies;
    let r2 = &latencies; // ...as many as you like, at the same time
    println!("avg={avg:?} len via r1={} first via r2={}", r1.len(), r2[0]);

    normalize(&mut latencies); // r1 and r2 are no longer used, so an exclusive borrow is allowed
    println!("normalized: {latencies:?}");

    let m = &mut latencies; // one writer...
    m.push(999);
    println!("after push: {latencies:?}"); // ...whose borrow ended at its last use

    latencies.push(latencies.len() as u32); // two-phase borrow: the argument is evaluated first
    println!("pushed its own length: {latencies:?}");

    let service = String::from("gateway");
    let winner = longest(&service, "db");
    println!("longest: {winner}");
}
```

```text
avg=Some(118.0) len via r1=5 first via r2=120
normalized: [45, 80, 120, 300]
after push: [45, 80, 120, 300, 999]
pushed its own length: [45, 80, 120, 300, 999, 5]
longest: gateway
```

Two details. `latencies.push(latencies.len() as u32)` compiles even though `push` needs `&mut latencies` and the
argument needs `&latencies`. [LANG] Method-call autoref creates a **two-phase borrow**: the mutable borrow is *reserved*
first, the arguments are evaluated (the shared read ends), and only then is the borrow *activated*. And `longest` needs a
lifetime annotation, `'a`, because it returns a reference that could come from either input, so the compiler can't infer
which one it borrows from. Part IV covers that properly.

**The errors are the rules.** Each one names the conflicting loans (all verified):

```text
error[E0506]: cannot assign to `limit` because it is borrowed
    let r = &limit;     // shared loan starts
    limit = 200;        // the owner tries to write while the loan is live
    println!("{r}");    // ...and the loan is used here

error[E0505]: cannot move out of `v` because it is borrowed
    let first = &v[0];  // a loan into v's buffer
    let n = consume(v); // moving v would move (and later free) the buffer under the loan
    println!("{first} {n}");
```

Together with E0502 (shared + mutable) and E0499 (two mutable) from Part I, that's the core set. Every one of them says
the same thing: *this loan is still live, and this other action would break what the loan guarantees.*

---

## Pass 2 · Systems level — *What a reference is, and what the rules buy*

### 4. Under the hood

**At run time, a reference is a pointer.** A `&u32` is 8 bytes on x86-64, and `&[T]` and `&str` are 16 (pointer +
length, Chapter 3.4). No borrow count, no lock, no flag exists at run time. [RUSTC] All borrow checking happens on MIR
and is then erased, like lifetimes and move/copy distinctions. A correct Rust program pays nothing at run time for
borrowing.

**But the rules are passed down to LLVM, and LLVM exploits them.** A function taking `&mut i32` and `&i32`, next to the
same function written with raw pointers (listing `ch03-04-noalias.rs`):

```rust,ignore
#[inline(never)]
pub fn add_twice(a: &mut i32, b: &i32) {
    *a += *b;
    *a += *b;
}

#[inline(never)]
pub unsafe fn add_twice_raw(a: *mut i32, b: *const i32) {
    unsafe {
        *a += *b;
        *a += *b;
    }
}
```

How rustc 1.98.1 describes the parameters to LLVM (release build):

```text
define void @...add_twice_raw(ptr noundef %a, ptr noundef readonly %b)

define void @...add_twice(ptr noalias noundef align 4 dereferenceable(4) %a,
                          ptr noalias noundef readonly align 4 dereferenceable(4) %b)
```

And the resulting x86-64:

```text
playground::add_twice:                        ; references: aliasing is impossible
	mov	eax, dword ptr [rsi]              ; load *b ONCE
	add	eax, eax                          ; 2 * b
	add	dword ptr [rdi], eax              ; *a += 2 * b, a single read-modify-write
	ret

playground::add_twice_raw:                    ; raw pointers: a and b MIGHT be the same address
	mov	eax, dword ptr [rdi]              ; load *a
	add	eax, dword ptr [rsi]              ; + *b
	mov	dword ptr [rdi], eax              ; store *a
	add	eax, dword ptr [rsi]              ; RELOAD *b: the store to *a might have changed it
	mov	dword ptr [rdi], eax              ; store *a again
	ret
```

Why the difference? If `a` and `b` point to the same `i32` with value 1, the correct answer is 4 (1+1=2, then 2+2=4), not
3. With raw pointers, the compiler can't rule out that case, so it must reload `*b` after writing `*a`. With `&mut i32`
and `&i32`, **aliasing XOR mutation guarantees they can't overlap**. `noalias` records that, and LLVM computes `2*b` once.

This is the same promise C's `restrict` makes, with two differences. In C it's rarely used, and **unchecked**: lying is
undefined behavior. In Rust it's on **every reference** and **checked by the compiler**. (History from Chapter 1.3:
mutable `noalias` was switched off for years because it exposed LLVM miscompilations that C's rare `restrict` never
triggered, and was re-enabled in 2021.) Rust's rules exist for safety, and they're also what makes pervasive `noalias`
possible.

**Reborrowing.** Passing a `&mut T` to a function that takes `&mut T` doesn't move your reference away. The compiler
inserts `&mut *r`, a **reborrow**: a new, shorter exclusive loan derived from `r`, which becomes usable again when the
reborrow ends. That's why you can call `normalize(xs)` twice on the same `xs: &mut Vec<u32>`. Part IV shows how
reborrows nest.

**Split borrows.** The checker tracks loans **per place**, field by field. `&mut self.accounts` and `&self.rate_bps` are
loans on *different* places, so they coexist. But calling a *method* `self.rate()` borrows **all of `*self`**, because
the signature `fn rate(&self)` says so (the checker looks at signatures, not bodies: Chapter 1.1). The debugging
exercise hits exactly this.

### 5. Memory

```text
 stack                                       heap
 ┌───────────────────────────┐
 │ latencies: Vec<u32>       │               ┌─────┬─────┬─────┬─────┬─────┐
 │   ptr ────────────────────┼─────────────► │ 120 │  45 │  45 │ 300 │  80 │
 │   cap, len                │               └─────┴─────┴─────┴─────┴─────┘
 ├───────────────────────────┤                  ▲
 │ r1: &Vec<u32> ────────────┼──► latencies     │   (a reference to the Vec HEADER on the stack)
 │ first: &u32 ──────────────┼──────────────────┘   (a reference INTO the heap buffer)
 │ xs: &[u32] = (ptr, len) ──┼──► buffer, 5         (a fat pointer: address + length)
 └───────────────────────────┘
```

A reference points at a **place**: a local's stack slot, a field, or an element in a heap buffer. That's why the rules
have to cover moves and reallocation as well as writes:

- **Moving a borrowed value is forbidden (E0505)** because a move can change its address. The stack slot is abandoned
  and the bytes are copied elsewhere. Chapter 2.3 noted that borrowing is what gives a value a *stable address*. Here's
  the other half: **while borrowed, the value may not move.**
- **Mutating a `Vec` while an element is borrowed is forbidden (E0502)**, because `push` may reallocate the buffer and
  leave `first` dangling. That's Chapter 1.1's bug, now with the whole mechanism in view.

### 6. CPU / OS

**The borrow rules mirror cache coherence.** Multicore CPUs keep caches consistent with a protocol (MESI and its
relatives) whose core invariant is **single-writer / multiple-reader** (SWMR): at any moment a cache line is either
*shared* by many cores for reading, or *owned* by one core for writing, never both. Aliasing XOR mutation is the same
invariant, enforced one level up, at compile time and per value:

```text
 hardware (per cache line, at run time)          Rust (per value, at compile time)
 ─────────────────────────────────────           ─────────────────────────────────
 Shared state:   many cores may read             &T:     many readers
 Modified/Exclusive: exactly one core writes     &mut T: exactly one writer, no readers
 a write invalidates other copies                a write requires that no other loans are live
```

That isn't a coincidence of vocabulary. It's why safe Rust can promise **no data races**. A data race is two threads
touching the same memory without ordering, at least one of them writing. The borrow rules make "two paths to the same
memory, one writing" unrepresentable in safe code, and `Send`/`Sync` (Part XI) extend the rule across threads. The
hardware still does its coherence work, but your program never *depends* on an unsynchronized race's outcome.

Two cost notes:

- **Indirection isn't free.** A reference is a pointer, and following it may be a cache miss. For small `Copy` values
  (`u64`, a two-float `Point`), pass by value: it fits in registers and has no aliasing questions at all.
- **`noalias` enables more than removing one reload.** It lets LLVM keep values in registers across loops and vectorize
  loops over `&mut [T]` without run-time overlap checks. In C, a loop over two pointers usually needs either `restrict`
  or a generated run-time check for overlap before vectorizing.

---

## Pass 3 · Architect level — *APIs that say what they do*

### 7. Trade-offs

**Choosing parameter types** is the most frequent ownership decision you'll make:

| Parameter | Means | Caller's cost | Use when |
|---|---|---|---|
| `&str`, `&[T]`, `&Path` | "I'll read it for the duration of this call" | None; accepts `String`, `Vec`, literals, sub-slices | Reading: the default |
| `&T` | Read a specific type | None | You need the full type's methods |
| `&mut T` / `&mut [T]` | "I'll modify it in place, then give it back" | Exclusive access for the call | In-place updates, filling buffers |
| `T` (owned) | "I'm keeping it, or consuming it" | A move (or a clone, if the caller still needs it) | Storing it, sending it to another thread, transforming it into something else |
| `&String`, `&Vec<T>` | Almost never right | Forces the caller to have exactly that type | Avoid: take `&str` / `&[T]` |

**Returning references** ties the result's lifetime to an input (`fn find(&self, ...) -> Option<&User>`, Chapter 1.1's
Registry). That's zero-copy, and it means the caller can't mutate the source while holding the result. Returning owned
data costs a copy and frees the caller. It's the same trade-off as Chapter 1.4's `RequestRef<'a>` vs `Request`.

### 8. Java comparison

Java references are **shared, mutable, and nullable, all at once**. Every aliasing bug in Java follows from that:

| Java tool | What it guarantees | Rust `&T` guarantees |
|---|---|---|
| `final` field | The *reference* won't be reassigned | The **value** won't change while you hold the loan |
| `Collections.unmodifiableList(list)` | *You* can't modify it through this view | *Nobody* can modify it (not even the owner) while the loan is live |
| `List.copyOf(list)` | An immutable snapshot, at the cost of a copy | The same freedom from surprises, with no copy |
| `synchronized` | Mutual exclusion at run time | (Across threads: `Mutex`, Part XI.) Within a thread, compile-time exclusivity |

The `unmodifiableList` row is the heart of it. A Java read-only view protects the *owner* from the *viewer*. A Rust
shared borrow also protects the *viewer* from the *owner*. That's why a Rust function taking `&[T]` never needs a
defensive copy: while it runs, the slice can't change.

> **Analogy limit.** "`&T` is a read-only Java reference" gets the read-only part right and misses two others. The
> referent is **frozen for everyone** during the loan, and the reference **can't outlive** the referent. Neither exists
> in Java: an unmodifiable view can observe changes the owner makes, and it can be held forever, since the GC keeps the
> target alive.

### 9. Production scenario

**Meridian's interest run and the split-borrow pattern.** The ledger batch job applies interest to every account. The
first version put the rate behind a getter method, which is idiomatic Java, and the Rust compiler rejected it (the
debugging exercise below). The fix taught the team a pattern they now use everywhere: **read what you need from
`self` before taking a mutable borrow of part of `self`, or access disjoint fields directly** (verified):

```rust
struct Account {
    balance: i64,
}

struct Bank {
    accounts: Vec<Account>,
    rate_bps: i64,
}

impl Bank {
    fn apply_interest(&mut self) {
        // Fix 1: borrow DISJOINT FIELDS directly: `self.accounts` mutably, `self.rate_bps` by copy.
        for acct in self.accounts.iter_mut() {
            acct.balance += acct.balance * self.rate_bps / 10_000;
        }
    }

    fn apply_interest_v2(&mut self) {
        // Fix 2: read what you need BEFORE taking the mutable borrow.
        let rate = self.rate_bps;
        for acct in &mut self.accounts {
            acct.balance += acct.balance * rate / 10_000;
        }
    }
}

fn main() {
    let mut bank = Bank { accounts: vec![Account { balance: 10_000 }], rate_bps: 250 };
    bank.apply_interest();
    bank.apply_interest_v2();
    println!("{}", bank.accounts[0].balance);
}
```

```text
10506
```

(10,000 → 10,250 → 10,506: integer cents, truncating.) Fix 2 has a design benefit beyond compiling. The rate is read
**once**, so the run can't observe a rate change halfway through a batch. The borrow checker pushed the code toward a
snapshot, which is the correct batch semantics anyway.

### 10. Failure scenario

**The report that changed order under its own feet.** A Java service computed a price list and handed the same `List`
to two components: a reporting component that kept a reference to render later, relying on arrival order, and a pricing
component that sorted the list in place to find the median. Both worked in isolation. Together, the report came out in
sorted order, silently, for months, until a customer asked why their statement's line items were in price order.

In Rust, the same sharing doesn't compile:

```rust,compile_fail
fn main() {
    let mut prices = vec![30, 10, 20];
    let report_view = &prices; // the reporting component keeps a view (it relies on arrival order)
    prices.sort(); // the pricing component sorts in place
    println!("report sees {:?}", report_view);
}
```

```text
error[E0502]: cannot borrow `prices` as mutable because it is also borrowed as immutable
```

The compiler forces the design question the Java code never asked: does the report need a **snapshot** (clone it,
explicitly), should the pricer sort a **copy** (`let mut sorted = prices.clone(); sorted.sort();`), or should the median
come from a **non-mutating** algorithm (`select_nth_unstable` on a copy, or a single pass over the data)? Each is fine.
The one design ruled out is the silent one.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part III).*

1. State the two borrowing rules. Why exactly these two? What would break if either were relaxed?
2. Why is `&T` `Copy` but `&mut T` not?
3. What does a reference cost at run time? What does borrow checking cost at run time?
4. Walk through the `add_twice` assembly. Why must the raw-pointer version reload `*b`, and what does `noalias` tell
   LLVM?
5. Why is moving a borrowed value forbidden (E0505)? Connect it to addresses.
6. How do the borrowing rules relate to the MESI single-writer/multiple-reader invariant, and to data-race freedom?
7. What is a two-phase borrow, and why does `v.push(v.len())` compile?
8. Why does `self.rate()` inside a loop over `self.accounts.iter_mut()` fail while `self.rate_bps` succeeds?

### 12. Exercises

- **Beginner.** For each function, pick the best parameter type and justify it: count words in a text; append a line
  to a log buffer; store a user's name in a registry; compute the median of prices without changing the caller's data.
- **Intermediate.** Write `fn largest_mut(xs: &mut [u32]) -> Option<&mut u32>` and use it to double the largest element.
  Then try to call it twice and print both results at once. Explain the error.
- **Advanced.** Write a function that swaps two elements of a slice given two indices *without* `slice::swap`, using
  `split_at_mut` to obtain two non-overlapping `&mut` references. Why can't you just take `&mut xs[i]` and `&mut xs[j]`?
- **Systems.** On the Playground, compile a loop `fn scale(dst: &mut [f32], src: &[f32], k: f32)` in release mode. Then
  compile the same loop with raw pointers. Compare: is there a run-time overlap check in the raw version? Is the
  reference version vectorized?
- **Architecture.** Review a public API you maintain in any language. For each parameter, write down whether it's read,
  modified, stored, or consumed. Where does the signature fail to say so? What would the Rust signature be?

### 13. Debugging exercise

```rust,compile_fail
struct Account {
    balance: i64,
}

struct Bank {
    accounts: Vec<Account>,
    rate_bps: i64,
}

impl Bank {
    fn rate(&self) -> i64 {
        self.rate_bps
    }

    fn apply_interest(&mut self) {
        for acct in self.accounts.iter_mut() {
            acct.balance += acct.balance * self.rate() / 10_000;
        }
    }
}
```

```text
error[E0502]: cannot borrow `*self` as immutable because it is also borrowed as mutable
```

1. Identify the two loans. Which one covers all of `*self`, and why? (Hint: what does the checker read, the body of
   `rate` or its signature?)
2. Why would accepting this be unsound *in general*, even though `rate` only reads a field that the loop doesn't touch?
   Imagine a future `rate()` implementation.
3. Give the two fixes from §9, plus a third that restructures the types (for example, moving `rate_bps` into a separate
   `Policy` struct passed by `&`). Which scales best as the struct grows?

### 14. Design exercise

**Meridian's pricing engine.** Quote requests read a `Catalog` (about 200 MB of prices and rules) continuously. Catalog
updates arrive every few seconds and must apply atomically: no quote may see a half-updated catalog. Using only this
chapter's tools (no threads yet), sketch the ownership and borrowing structure:

- Who owns the current `Catalog`?
- What does a quote computation hold, and for how long?
- How is an update applied without violating aliasing XOR mutation while quotes are in progress?

Compare two designs: "build a new catalog and swap it in between requests" vs "apply updates in place during a pause."
Then note what changes once quotes run on many threads (Part XI will use `Arc` for exactly this). Which of your
single-threaded design decisions survive?
