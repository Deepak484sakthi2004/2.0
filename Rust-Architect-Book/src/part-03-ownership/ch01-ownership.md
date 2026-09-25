# Chapter 3.1 — Ownership: One Owner, One Drop

> **Where this sits:** Part III · Ownership · chapter 1 of 6
> **Prerequisites:** Part I (the central question), Part II (bindings, receivers, layout).
> **After this chapter you can:** state the ownership rules precisely; draw the ownership tree of any value; explain
> how the compiler turns ownership into drop calls (including drop flags, which you'll read in real MIR); and reason about
> what dropping a large structure costs in production.

---

## Pass 1 · User level — *The rules*

### 1. Problem

Chapter 1.1 posed the question every memory-management design answers: *who is responsible for freeing this memory,
and how do we know nobody is still using it?* Part II answered it implicitly a dozen times (a `self` receiver consuming
a builder, a `Summary` copying out a path before the line buffer was reused) without stating the model.

You need the model stated precisely, because in Rust you're expected to answer the question **by reading the code**: for
every value, who owns it, and when does it die. In Java the GC answers it at run time, and you rarely think about it
until a heap dump forces you to. In Rust the compiler checks your answer at every line. Engineers who never learn the
model argue with the compiler. Engineers who do learn it find the compiler is just checking their own reasoning.

### 2. Mental model

**The rules** [LANG]:

1. **Every value has exactly one owner at any time.**
2. **Ownership can be transferred (moved).** After a move, the previous owner can't use the value.
3. **When the owner goes away, the value is dropped, exactly once.** "Goes away" means the owning variable goes out of
   scope, the owning structure is itself dropped, or the owning place is overwritten.

**What can be an owner:**

| Owner | Owns until... |
|---|---|
| A local variable or function parameter | the end of its scope (or until the value is moved out) |
| A struct or enum field | its containing value is dropped |
| A collection element (`Vec<T>` owns its `T`s) | it's removed, or the collection is dropped |
| A `Box<T>` (owns one heap value) | the box is dropped |
| A temporary (`make_string().len()`) | the end of the enclosing statement (Chapter 3.5 has the exact rules) |
| A `static` | forever (the program's lifetime) |

Because every value has exactly one owner, **ownership forms a tree** (strictly, a forest). The roots are locals,
statics, and temporaries. Dropping a node drops its whole subtree:

```text
 main's stack frame
 └── b: Order #2                          (root: a local)
     ├── customer: Noisy("customer B")    (owned by a field)
     └── lines: Vec<Noisy>                (a field that owns a heap buffer...)
         └── [0] Noisy("line B1")         (...which owns its elements)
```

Two relationships are **not** ownership, and each gets its own chapter. **Borrowing** (Chapter 3.3) is temporary
access that never outlives the owner and never frees anything. **Shared ownership** (Chapter 3.6) is an explicit
opt-in (`Rc`, `Arc`) where a reference count decides who drops last. Everything else is the tree.

### 3. Rust code

An ownership tree, a transfer, and two drops (verified):

```rust
struct Noisy(&'static str);

impl Drop for Noisy {
    fn drop(&mut self) {
        println!("  drop {}", self.0);
    }
}

struct Order {
    id: u32,
    customer: Noisy,
    lines: Vec<Noisy>,
}

impl Drop for Order {
    fn drop(&mut self) {
        println!("  drop Order #{} (its fields follow)", self.id);
    }
}

fn ship(order: Order) -> u32 {
    // `ship` OWNS the order now
    println!("shipping #{}", order.id);
    order.id
} // `order` goes out of scope here: the whole ownership tree is dropped

fn main() {
    let a = Order { id: 1, customer: Noisy("customer A"), lines: vec![Noisy("line A1"), Noisy("line A2")] };
    let b = Order { id: 2, customer: Noisy("customer B"), lines: vec![Noisy("line B1")] };
    let shipped = ship(a); // ownership of `a` moves into `ship`
    println!("shipped #{shipped}; main ends");
} // `b` goes out of scope here
```

```text
shipping #1
  drop Order #1 (its fields follow)
  drop customer A
  drop line A1
  drop line A2
shipped #1; main ends
  drop Order #2 (its fields follow)
  drop customer B
  drop line B1
```

Read the output as a trace of the tree:

- Order #1 is dropped **inside `ship`**, before `main` prints anything else, because `ship` became its owner, and
  `ship`'s scope ended first.
- For each order, the struct's own `Drop::drop` runs first, then its fields **in declaration order** (`customer`, then
  `lines`), then a `Vec`'s elements **front to back**. [LANG] Those orders are specified by the language.
- `main` never dropped `a`. Once `a` was moved, the obligation to drop it moved with it.

**Ownership is also an API contract.** A method that takes `self` consumes the value, so it can't be called twice:

```rust,compile_fail
struct Connection {
    id: u32,
}

impl Connection {
    fn close(self) {
        // takes ownership: the connection cannot be used after this
        println!("closing connection {}", self.id);
    }
}

fn main() {
    let conn = Connection { id: 7 };
    conn.close();
    conn.close();
}
```

```text
error[E0382]: use of moved value: `conn`
  --> src/main.rs:16:5
   |
14 |     let conn = Connection { id: 7 };
   |         ---- move occurs because `conn` has type `Connection`, which does not implement the `Copy` trait
15 |     conn.close();
   |          ------- `conn` moved due to this method call
16 |     conn.close();
   |     ^^^^ value used here after move
   |
note: `Connection::close` takes ownership of the receiver `self`, which moves `conn`
```

In Java, "close it twice" is a run-time question (`IllegalStateException`, or silently fine, depending on the class).
In Rust it's a type error, because closing *consumes* the connection.

---

## Pass 2 · Systems level — *How ownership becomes code*

### 4. Under the hood

The compiler turns ownership into code in three steps.

**1. Every owning place gets a drop obligation.** [RUSTC] During MIR building, each local that holds a value needing
cleanup is scheduled for a `drop` at the end of its scope, on the normal path *and* on every unwind path (Chapter 3.5
shows the cleanup blocks).

**2. Moves are tracked statically, by control flow.** The borrow checker and **drop elaboration** track, for each
point in the function, whether each place (and each field of each place) is initialized, moved-out, or maybe-moved.
When the answer is certain on every path, the compiler simply emits the drop or omits it. When it **depends on run-time
control flow**, it adds a hidden boolean, a **drop flag**. Here's a real one, from the debug MIR of:

```rust,ignore
pub fn maybe_consume(flag: bool) {
    let s = String::from("hi");
    if flag {
        consume(s);
    }
} // `s` must be dropped here ONLY if it was not moved
```

```text
fn maybe_consume(_1: bool) -> () {
    let _2: std::string::String;        // s
    let mut _5: bool;                   // ← the drop flag for s
    ...
    bb0: {
        _5 = const false;
        _5 = const true;                // s is initialized: flag on
        _2 = <String as From<&str>>::from(const "hi") -> [return: bb1, unwind continue];
    }
    bb1: {
        switchInt(copy _1) -> [0: bb3, otherwise: bb2];     // if flag
    }
    bb2: {
        _5 = const false;               // s is about to be moved: flag off
        _4 = move _2;
        _3 = consume(move _4) -> [return: bb3, unwind continue];
    }
    bb3: {
        switchInt(copy _5) -> [0: bb4, otherwise: bb5];     // end of scope: still own it?
    }
    bb4: {
        _5 = const false;
        return;
    }
    bb5: {
        drop(_2) -> [return: bb4, unwind continue];         // yes → drop it
    }
}
```

That's the whole mechanism of "exactly once," visible: set on initialization, cleared on move, checked at scope end.
In release builds the flag disappears. LLVM merges it into the branch on `flag` itself (rustc 1.98.1, x86-64):

```text
playground::maybe_consume:
	...
	call	qword ptr [rip + __rustc::__rust_alloc@GOTPCREL]   ; String::from("hi"): allocate 2 bytes
	...
	mov	word ptr [rax], 26984          ; write "hi" (0x6968) into the buffer
	test	bl, bl                         ; if flag
	je	.LBB0_4
	...                                    ; moved path: build the String and call consume(); NO free here
	call	qword ptr [rip + playground::consume@GOTPCREL]
	...
	ret
.LBB0_4:                                       ; not-moved path: drop(s) is a direct dealloc
	...
	jmp	qword ptr [rip + __rustc::__rust_dealloc@GOTPCREL]
```

No boolean is stored, and the drop for the not-moved path is a tail jump into the deallocator. `drop_in_place::<String>`
was inlined down to a single `__rust_dealloc` call.

**3. Drop glue.** [RUSTC] For every type that needs cleanup, the compiler generates a function,
`core::ptr::drop_in_place::<T>`, called **drop glue**. It calls the type's `Drop::drop` (if there is one) and then
recursively drops each field. `drop(_2)` in MIR becomes a call to `drop_in_place::<String>`, which frees the heap
buffer. Types with no cleanup (integers, `&T`, `[u64; 1024]`) have no drop glue at all, and dropping them is literally
nothing (Chapter 3.5 prints `needs_drop` for several types).

> **What actually happens?** When `ship(a)` is called, what does `main`'s frame still contain? [RUSTC] The bytes of `a`
> may still be sitting in `main`'s stack slot, since a move is a bitwise copy (Chapter 3.2). But the compiler has
> recorded that `a` is moved, so no code reads those bytes and no drop is emitted for them. Ownership is a fact the
> *compiler* knows. It isn't a bit stored in the object. Contrast a C++ moved-from `std::string`, which really is a live
> object whose destructor still runs.

### 5. Memory

The ownership tree of `vec![String::from("customer-0"), ...]` in memory, and what dropping it does:

```text
 stack (main's frame)            heap
 ┌──────────────────────┐        ┌──────────────────────────────────────────────┐
 │ names: Vec<String>   │        │ [0] String{ptr,cap,len} │ [1] ... │ [999] ... │  one buffer: 1000 × 24 B
 │   ptr ───────────────┼──────► └───┬────────────────────────────────┬──────────┘
 │   cap = 1000         │            │                                │
 │   len = 1000         │            ▼                                ▼
 └──────────────────────┘        "customer-0"   ...  (1000 separate small buffers)   "customer-999"

 drop(names):  drop_in_place::<Vec<String>>
                 → for each element: drop_in_place::<String>  → free(buffer)     × 1000
                 → free(the Vec's own buffer)                                      × 1
```

This isn't a model. It's measured. Listing `ch01-02-frees.rs` installs a counting allocator (a `GlobalAlloc` that
counts calls before forwarding to the system allocator; Part XV explains the mechanism) and drops two vectors:

```text
dropping a Vec<u64> of 1000:    1 free(s)
dropping a Vec<String> of 1000: 1001 free(s)
```

Same length, very different cost. A `Vec<u64>` is one allocation because its elements are plain bytes inside the
buffer. A `Vec<String>` is 1,001 allocations because each element owns its own heap buffer. **The shape of the ownership
tree is the shape of the free list.**

### 6. CPU / OS

**Dropping is work, and it happens on the thread that drops.** Each free is an allocator call. Tens of nanoseconds is a
reasonable order of magnitude for a fast thread-caching allocator, more under contention. Walking a large tree also
touches memory that may be cold, which adds cache misses. For a large structure it adds up. Listing
`ch01-05-deferred-drop.rs` builds a `HashMap<u64, String>` with a million entries and times dropping it. One run on the
Playground's machine (release build; your numbers will differ):

```text
inline drop on this thread: 60.552929ms
handoff to a dropper thread: 73.84µs
```

About 60 ms to drop a million-entry map on the calling thread, against about 74 µs to hand the map to another thread
and let *that* thread pay. If the calling thread is serving requests, that 60 ms lands directly in your tail latency.
Treat it as a measured *shape*. Measure your own structures (Part XX).

**Freeing isn't returning memory to the OS.** [LIB] [OS] `free` hands memory back to the *allocator*, which usually
keeps it for reuse. The process's resident set size (RSS) often doesn't shrink after a big drop. glibc returns memory
to the kernel only in some cases (and `malloc_trim` forces it); jemalloc and mimalloc have their own decay policies.
"We dropped the cache but RSS stayed at 4 GB" is usually the allocator, not a leak.

---

## Pass 3 · Architect level — *Who should own what?*

### 7. Trade-offs

Every value's lifetime is managed by one of a small set of strategies. Choosing among them *is* Rust data design:

| Strategy | Who frees | When | Cost | Use when |
|---|---|---|---|---|
| **Single owner** (the tree) | The owner | Deterministically, at scope end | None at run time | The default, for almost everything |
| **Borrowing** (3.3) | Nobody; the borrower never frees | n/a | None; lifetimes are checked at compile time | Temporary access |
| **Shared ownership** (`Rc`/`Arc`, 3.6) | Whoever drops the *last* handle | When the count hits zero | A count per handle; atomic for `Arc` | Genuinely shared lifetimes (caches, graphs, cross-thread data) |
| **Arena / bulk** (3.6) | The arena, all at once | When the arena is dropped | One big free; no per-node frees | Many objects with one shared lifetime (a request, a parse, a graph) |
| **Deliberate leak** (`Box::leak`) | Nobody; the OS at exit | Never | Memory for the rest of the process | Startup config and tables that live forever, turned into `&'static` |

The last row isn't a joke. `Box::leak(Box::new(config))` gives you a `&'static Config` that any thread can read with
no reference counting, and for data that genuinely lives as long as the process it's the cheapest correct choice. The
leak is a *design decision*, not an accident.

### 8. Java comparison

Java has no concept of an owner. An object lives while *any* reference to it is reachable, and the GC decides when
reachability ends. That has two consequences worth being honest about:

- **Java is better at dropping huge structures.** Replace a million-entry map in Java and the old one becomes garbage,
  reclaimed *concurrently* by GC threads. The request thread pays almost nothing. In Rust, whichever thread drops it
  pays (§6), unless you design around that (§9). This is a real advantage of tracing GC.
- **Java has ownership anyway, for resources, but only as convention.** Who closes this `InputStream`? The caller that
  opened it, or the method it was passed to? Javadoc says, if you're lucky. `try-with-resources` fixes the *when*, but
  not the *who*.

> **Analogy limit.** "Ownership is like a Java object that exactly one variable references" is useful for a few
> minutes. It breaks at once, because Java can't *prevent* a second reference, and the point of Rust ownership is that
> the second owner can't exist. A closer Java intuition is a **resource you're responsible for closing**, applied to
> *every* value, memory included, with the compiler checking that the responsibility is handed over explicitly and
> never duplicated.

### 9. Production scenario

**Meridian's route-table refresh.** Every ten minutes the gateway rebuilds its routing and tenant table (about a
million entries) and swaps it in. The first Rust version replaced the table on the request thread that noticed the
refresh:

```rust,ignore
*self.table = new_table;   // assigning over the old table DROPS it, right here, on a request thread
```

Every ten minutes one request paid tens of milliseconds for a million frees (the shape measured in §6), a p99.9 spike
that had no cause visible in application logic. Three designs, each with a trade-off:

1. **A dropper thread.** Send the old table over a channel to a background thread that drops it. The request thread
   pays a channel send. This is the direct fix.
2. **`Arc` swap** (Part XI): readers hold `Arc<Table>` snapshots, and the refresher swaps in a new `Arc`. The catch:
   the old table is dropped by **whichever thread releases the last `Arc`**, which may again be a request thread. So
   you still need design 1 behind it, for example by having the refresher keep the old `Arc` and hand it to the dropper
   once readers are done.
3. **An arena-backed table.** Allocate all entries from one arena, so dropping is a handful of large frees instead of
   a million small ones. This changes the cost instead of moving it.

Meridian shipped design 2 with design 1 behind it. It also added a metric that neither Java nor Rust gives you for free:
**time spent in drop, per structure**.

### 10. Failure scenario

**Whose stream is it?** A Java ingestion service passes an open `InputStream` from the HTTP layer into a parser
library:

```java
InputStream body = request.getInputStream();
Batch batch = parser.parse(body);        // the library closes the stream when done (documented in v2.1, not in v2.0)
auditLog.copyRaw(body);                  // IOException: Stream closed. It worked in v2.0.
```

The library's ownership convention changed in a minor release, and nothing in the types said so. In the opposite
version of this bug, where neither side closes, the service leaks a file descriptor per request until
`Too many open files` takes it down.

In Rust the signature *is* the ownership documentation, and the compiler enforces it:

```rust,ignore
fn parse(body: Body) -> Batch             // takes ownership: parse drops (closes) it; the caller can't use it after
fn parse(body: &mut Body) -> Batch        // borrows: the caller keeps it, the caller's scope closes it
```

Changing from the second to the first is a **breaking change the compiler reports at every call site** (the caller's
later use becomes E0382, as with `conn.close()` above). It can't slip through in a minor release unnoticed.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part III).*

1. State Rust's ownership rules precisely. What exactly counts as "the owner goes away"?
2. Why does ownership form a tree, and what are the roots? Which Rust features let you step outside the tree, and at
   what cost?
3. Explain drop flags. When does the compiler need one, and what does it cost in a release build?
4. What is drop glue? Which types have none, and why does that matter for performance?
5. Dropping a `Vec<u64>` of 1,000 elements costs one free; a `Vec<String>` costs 1,001. Generalize: how do you predict
   the cost of dropping any structure?
6. Why can dropping a large structure cause a latency spike, and what are three ways to prevent it?
7. Why doesn't RSS always go down after you drop a large structure?
8. Compare how a Java API and a Rust API communicate "who closes this resource." Which failure modes does each allow?

### 12. Exercises

- **Beginner.** Add a third field `notes: Option<Noisy>` to `Order` and predict the new drop output for `Some(...)` and
  for `None`. Then run it.
- **Intermediate.** Write `fn split_order(order: Order) -> (Order, Order)` that moves half of the lines into a new
  order. Trace which `Noisy` values are dropped when, and why none is dropped twice.
- **Advanced.** Get the MIR for a function where a value is moved inside a loop *conditionally* (for example, `if i == 3
  { consume(s); break; }`). Find the drop flag and explain every place it's set and tested.
- **Systems.** Run the deferred-drop listing locally with 1M, 5M, and 10M entries. Plot drop time against entries.
  Is it linear? Now switch the value type from `String` to `u64` and explain the difference.
- **Architecture.** For a service you know, list its three largest in-memory structures. For each, say which thread
  would drop it in a Rust port and whether that thread is latency-sensitive. Propose a design for any that are.

### 13. Debugging exercise

The `Connection` listing above fails with E0382. A reviewer suggests changing `close(self)` to `close(&mut self)`, "so
it compiles."

1. With `close(&mut self)`, the code compiles. What *bug* does that reintroduce? Consider a connection that's used after
   being closed.
2. With `close(&mut self)`, what must the type now track internally, and what must every other method check?
3. Which signature expresses the domain better, and when would `close(&mut self)` actually be right? (Hint: think
   about closing and reopening, and about pools.)

### 14. Design exercise

**The ownership map of Meridian's request pipeline.** A gateway request flows through: TLS decryption into a receive
buffer → header parsing → authentication (looks up the tenant in a shared tenant table) → routing → a pooled upstream
connection → response streaming back to the client.

Draw the ownership tree at three moments: during header parsing, during the upstream call, and during response
streaming. For each value, mark whether it's **owned** (by whom), **borrowed** (from whom), or **shared** (`Arc`).
Identify every handoff where ownership moves between components, and every value that must outlive the request (the
tenant table, pooled connections). Justify each choice, and name the one you're least sure about.
