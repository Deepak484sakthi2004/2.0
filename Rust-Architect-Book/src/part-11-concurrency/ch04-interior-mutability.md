# Chapter 11.4 — Interior Mutability: Cell, RefCell, OnceCell, UnsafeCell

> **Where this sits:** Part XI · Concurrency · chapter 4 of 7
> **Prerequisites:** Chapter 4.1 (`UnsafeCell` and the interior-mutability table), Chapter 3.6 (`RefCell`), Chapters
> 11.2–11.3.
> **After this chapter you can:** explain why every way of mutating through `&T` is built on `UnsafeCell` and what
> Miri reports when it isn't; pick among `Cell`, `RefCell`, `OnceCell`/`OnceLock`, `LazyLock`, atomics, locks, and
> `ArcSwap` from three questions (who writes, how often, from how many threads); read their sizes and costs; and publish
> read-mostly data to many threads without locks, and without freeing it on a request thread.

---

## Pass 1 · User level — *Changing things through a shared reference*

### 1. Problem

`&self` methods that need to change something are everywhere: a request context counting cache hits, a lazily loaded
table, a config that's replaced every few minutes, a metric incremented by every thread. Aliasing XOR mutation says a
shared reference can't write. **Interior mutability** is the set of types that let it write anyway, each with a
different argument for why that's still safe.

Chapter 4.1 introduced the family. This chapter finishes the job for multiple threads: the one-time-initialization
types (`OnceLock`, `LazyLock`), the jump from per-request `Cell`s to process-wide atomics that Chapter 4.1's production
scenario promised, and lock-free publication with `ArcSwap` for Chapter 3.1's route table and Chapter 3.3's pricing
catalog.

### 2. Mental model

**One primitive, many strategies.** Every type that mutates through `&T` wraps an `UnsafeCell<T>` [LANG], the only
legal way to say "this memory may change behind a shared reference". Each type then re-establishes safety in its own
way:

| Type | How it keeps aliasing XOR mutation | Threads | Failure mode |
|---|---|---|---|
| `Cell<T>` | Never hands out a reference to the inside: `get` copies, `set`/`replace` swap | one | none |
| `RefCell<T>` | Counts borrows at run time | one | panic: "already borrowed" |
| `OnceCell<T>` / `OnceLock<T>` | Write once; after that, only `&T` | one / many | init panic: `OnceLock` stays empty |
| `LazyCell<T>` / `LazyLock<T>` | `OnceCell`/`OnceLock` + the init closure | one / many | init panic **poisons** `LazyLock` |
| Atomics (`AtomicU64`, ...) | Each operation is indivisible in hardware | many | none (but ordering rules, Part XIV) |
| `Mutex<T>` / `RwLock<T>` | Blocks until access is exclusive (or shared-read) | many | contention, deadlock, poisoning |
| `ArcSwap<T>` (crate) | Replace the whole value atomically; readers keep the old one | many | old versions stay alive while referenced |

Three questions choose among them: **Who writes?** (one thread or many) **How often?** (once, rarely, constantly)
**What's the invariant?** (a single number, a whole value, or several fields together)

### 3. Rust code

**Write once, read forever: `OnceLock` and `LazyLock`** (listing `ch04-02-once-and-lazy.rs`, stable since 1.70 and
1.80 [VERSION]):

```rust,ignore
static INIT_RUNS: AtomicU32 = AtomicU32::new(0);
static PRICE_TABLE: OnceLock<HashMap<&'static str, u64>> = OnceLock::new();

fn prices() -> &'static HashMap<&'static str, u64> {
    PRICE_TABLE.get_or_init(|| {
        INIT_RUNS.fetch_add(1, Ordering::Relaxed);
        thread::sleep(std::time::Duration::from_millis(20)); // a slow load: the race window is wide open
        HashMap::from([("basic", 900), ("pro", 2900)])
    })
}

static REGION: LazyLock<String> = LazyLock::new(|| std::env::var("REGION").unwrap_or_else(|_| "eu-west-1".into()));

static BROKEN: LazyLock<u32> = LazyLock::new(|| panic!("config file missing"));
```

```text
8 threads saw [2900, 2900, 2900, 2900, 2900, 2900, 2900, 2900]; initializer ran 1 time(s)
REGION = eu-west-1
  [panic] first attempt failed
OnceLock after a panicking init: is_err = true, get() = None
OnceLock retry: 7
  [panic] config file missing
LazyLock access #1: panicked = true
  [panic] LazyLock instance has previously been poisoned
LazyLock access #2: panicked = true
```

Eight threads raced into `get_or_init` during a 20 ms initialization, and **the initializer ran once**. The other seven
waited and then saw the same value. The last lines show the two failure behaviors, which are different on purpose. A
`OnceLock` whose initializer panicked is still empty, so the next caller may retry. A `LazyLock` whose initializer
panicked is **poisoned for good**: its closure was consumed, and every later access panics. §10 is what that means in a
service.

**From per-request `Cell`s to process-wide atomics** (Chapter 4.1's production scenario, finished; listing
`ch04-03-cell-to-atomic.rs`, excerpt):

```rust,ignore
/// Per request, owned by one thread: Cell is enough, and it's free.
#[derive(Default)]
struct RequestStats {
    cache_lookups: Cell<u32>,
    cache_hits: Cell<u32>,
    upstream_calls: Cell<u32>,
}

/// Process-wide: shared by every worker thread.
#[derive(Default)]
struct GlobalStats {
    requests: AtomicU64,
    cache_lookups: AtomicU64,
    cache_hits: AtomicU64,
    upstream_calls: AtomicU64,
}

impl Drop for RequestContext<'_> {
    /// Fold the request's counters into the global ones: 4 atomic adds per request, whatever happened inside.
    fn drop(&mut self) {
        let g = self.global;
        g.requests.fetch_add(1, Ordering::Relaxed);
        g.cache_lookups.fetch_add(self.stats.cache_lookups.get().into(), Ordering::Relaxed);
        g.cache_hits.fetch_add(self.stats.cache_hits.get().into(), Ordering::Relaxed);
        g.upstream_calls.fetch_add(self.stats.upstream_calls.get().into(), Ordering::Relaxed);
    }
}
```

```text
requests=10000 lookups=40000 hits=26666 upstream=13334
```

Inside a request, helpers update `Cell`s through `&RequestContext`: plain loads and stores, no atomics, and the
compiler guarantees the context never leaves its thread (`Cell` is `!Sync`). When the request ends, `Drop` folds the
counts into the global atomics **once**. The shared cache lines are touched 4 times per request instead of once per
event. That design is the one Chapter 4.1's E0277 pointed toward.

**Building your own: a `Cell` on `UnsafeCell`** (listing `ch04-06-mycell.rs`, which passes under Miri):

```rust
use std::cell::UnsafeCell;

pub struct MyCell<T> {
    value: UnsafeCell<T>,
}

impl<T: Copy> MyCell<T> {
    pub fn new(value: T) -> Self {
        MyCell { value: UnsafeCell::new(value) }
    }

    pub fn get(&self) -> T {
        // SAFETY: MyCell is !Sync (UnsafeCell is !Sync, and we add no impl), so only this thread can reach it.
        // No reference into the interior is ever handed out, so no `&T` can observe the write in `set`.
        unsafe { *self.value.get() }
    }

    pub fn set(&self, value: T) {
        // SAFETY: as in `get`: single thread, and nobody holds a reference to the interior.
        unsafe { *self.value.get() = value }
    }
}

fn main() {
    let hits = MyCell::new(0u32);
    let a = &hits;
    let b = &hits; // two shared references...
    a.set(a.get() + 1);
    b.set(b.get() + 1); // ...both mutating: legal, because the mutation goes through UnsafeCell
    println!("hits = {}", hits.get());
}
```

```text
hits = 2
```

The two `SAFETY` comments are the entire design of `std::cell::Cell`. `!Sync` rules out other threads, and "no
references to the interior" rules out a reader seeing a write mid-borrow. The `T: Copy` bound makes `get` possible
without handing out a reference.

---

## Pass 2 · Systems level — *The primitive, the layouts, the costs*

### 4. Under the hood

**Why `UnsafeCell` is not optional** [LANG]. Mutating memory that is only reachable through a shared reference, and
not inside an `UnsafeCell`, is undefined behavior. It isn't a style rule, because the compiler optimizes on it: rustc
tells LLVM that a `&T` parameter points to memory that is `noalias` and `readonly` for the call (verified in Chapter
4.1's IR). Write the same "cell" without `UnsafeCell` and Miri catches it (listing `ch04-07-no-unsafecell.rs`):

```rust,ignore
pub fn set(&self, value: T) {
    let p: *mut T = (&raw const self.value).cast_mut();
    // SAFETY: none. `&self` promises the value is frozen; writing through it is UB.
    unsafe { p.write(value) }
}
```

```text
error: Undefined Behavior: attempting a write access using <443> at alloc212[0x0], but that tag only grants
       SharedReadOnly permission for this location
help: <443> was created by a SharedReadOnly retag at offsets [0x0..0x4]
  --> src/main.rs:14:25
14 |         let p: *mut T = (&raw const self.value).cast_mut();
```

"SharedReadOnly" is Miri's Stacked Borrows model (Part XV) saying what the language says: a pointer derived from `&T`
can read, never write, unless the bytes are inside an `UnsafeCell`. `UnsafeCell<T>` does three things, and only three
[LANG] [RUSTC]:

1. It makes writes through `&UnsafeCell<T>` (via the raw pointer from `.get()`) legal.
2. It removes the `noalias`/`readonly` promises for those bytes, so the optimizer reloads them (Chapter 4.1's `add eax,
   dword ptr [rdi]` vs `add eax, eax`).
3. It is `!Sync`. Any type containing it is `!Sync` until someone writes an `unsafe impl Sync` and proves the
   synchronization, as `Mutex` and the atomics do.

It does **not** synchronize anything. That's what the types built on it are for.

**How `OnceLock` runs its initializer once** [LIB]. A `OnceLock<T>` is a `std::sync::Once` (a futex-based state word:
incomplete, running, complete, poisoned) next to an `UnsafeCell<MaybeUninit<T>>`. The first caller moves the state to
*running* with a compare-and-swap and runs the closure. The others see *running* and sleep on the futex. On success the
winner writes the value, sets *complete* (with Release ordering, Part XIV), and wakes everyone. Later calls see
*complete* with an Acquire load and return `&T`: after initialization, reading a `OnceLock` costs one atomic load.
`LazyLock` adds the closure and, when the closure panics, records that it can't try again.

**How `ArcSwap` reads without locks** [LIB]. `ArcSwap<T>` holds an atomic pointer to an `Arc<T>`'s data. A naive
lock-free `load` would read the pointer and increment the refcount, but between those two steps a writer might swap the
pointer and drop the last reference: a use-after-free. `arc-swap` solves this with **debts**. A reader records "I'm
using pointer P" in a small per-thread slot, and a writer that swaps P out pays those debts by incrementing the
refcount on the readers' behalf before releasing its own reference. In the common case, a `load()` touches only its own
thread's slot and never the shared refcount. That's why it's fast (§6).

### 5. Memory

The family, measured (listing `ch04-01-family-sizes.rs`, rustc 1.98.1, x86-64; [RUSTC]/[LIB] facts, except
`UnsafeCell<T>` having `T`'s layout, which is documented):

```text
u64                                  8 bytes
UnsafeCell<u64>                      8 bytes
Cell<u64>                            8 bytes
RefCell<u64>                        16 bytes
OnceCell<u64>                       16 bytes
OnceCell<String>                    24 bytes
AtomicBool                           1 bytes
AtomicU64                            8 bytes
OnceLock<u64>                       16 bytes
Mutex<()>                            8 bytes
Mutex<u64>                          16 bytes
RwLock<()>                          12 bytes
RwLock<u64>                         24 bytes
parking_lot::Mutex<()>               1 bytes
parking_lot::Mutex<u64>             16 bytes
parking_lot::RwLock<u64>            16 bytes
Mutex<[u8; 64]>                     72 bytes
```

What the numbers say:

- **`Cell` and atomics add nothing.** Their cost is in *instructions*, not bytes.
- **`RefCell` adds a borrow counter** (an `isize`): 8 bytes plus alignment.
- **`OnceCell<String>` is 24, the same as `String`**, while `OnceCell<u64>` is 16. "Uninitialized" hides in
  `String`'s niche (Chapter 5.2) but needs a separate tag next to a `u64`.
- **std's `Mutex` adds 8 bytes** (a `u32` futex word, a poison `bool`, and padding), and `RwLock` adds 12 (two `u32`s
  plus poison). **`parking_lot::Mutex<()>` is 1 byte**: its lock is a single `AtomicU8`, and waiting threads are parked
  in a global hash table keyed by the lock's address [LIB]. That lets you put a lock in every element of a large array.
- **`Mutex<[u8; 64]>` is 72 bytes, which is more than one cache line.** Chapter 11.7's false-sharing measurements
  depend on this kind of arithmetic.

`ArcSwap`'s memory cost is **time-shaped**: every version that some reader still holds stays alive. With a steady
stream of updates and a slow reader, several versions of a 200 MB catalog can coexist. The listing in §9 shows how to
bound that, and how to control *where* the old version is freed.

### 6. CPU / OS

**The read path of shared, rarely-updated data.** It's the most common interior-mutability decision in services
(config, routing tables, feature flags, catalogs). Four reader threads, one million reads each, where every read needs a
consistent snapshot it can keep using (listing `ch04-05-read-path-cost.rs`, release, one run):

```text
Mutex<Arc<Table>>: lock, clone Arc        218.9 ns per read
RwLock<Arc<Table>>: read lock, clone Arc  259.1 ns per read
ArcSwap<Table>: load()                      7.8 ns per read
no sharing needed (baseline): &Table        2.1 ns per read
```

Both lock-based versions write to **two shared cache lines per read**: the lock word (or reader count), and the `Arc`
refcount. With four cores doing that, the lines bounce continuously, and the "read" costs as much as a contended write.
`RwLock` is *slower* than `Mutex` here, because a read lock is a write to the reader count and the critical section is
too short to benefit from parallel readers (Chapter 11.3 §7). `ArcSwap::load` writes only to a thread-local debt slot,
so no line is shared for writing, and it runs within 4× of an unsynchronized pointer read. That's about 28× faster than
the `Mutex` version.

On x86-64, a `Relaxed` or `Acquire` atomic load is an ordinary `mov` [CPU]. The cost of synchronization is almost
entirely in **writes to shared lines**, which is why every fast design in this Part minimizes them: `Cell`s folded once
per request (§3), `ArcSwap` for readers, per-thread counters (Chapter 11.7).

---

## Pass 3 · Architect level — *Choosing, and publishing*

### 7. Trade-offs

**The decision, as questions:**

```text
Is the data touched by one thread only?
  └─ yes → Cell (Copy values) · RefCell (borrowing needed; a conflict is a bug → panic) · OnceCell/LazyCell
  └─ no ↓
Is it written exactly once (at startup or first use)?
  └─ yes → OnceLock / LazyLock (reads cost one atomic load)   — but prefer explicit init at startup (§10)
  └─ no ↓
Is it replaced as a whole, and read far more often than written?
  └─ yes → ArcSwap<T> (lock-free reads; writers build a new T off to the side)
  └─ no ↓
Is each piece of state a single number, updated independently?
  └─ yes → atomics (Relaxed for counters and statistics; Part XIV for anything that publishes other data)
  └─ no ↓
Are several fields updated together under one invariant?
  └─ yes → Mutex<T> (or RwLock<T> after measuring; or sharding, Chapter 11.7)
```

**Global state vs passing it in.** `LazyLock` statics are convenient and make testing harder: tests share the static,
initialization order is implicit, and a failed initialization poisons it for the whole process. For anything that
depends on the environment (configuration, connections), prefer building the value in `main` and passing
`Arc<Config>` down. Keep `LazyLock` for pure, infallible computations (a compiled regex, a lookup table).

### 8. Java comparison

| Java idiom | Rust counterpart | Note |
|---|---|---|
| Initialization-on-demand holder class | `LazyLock` / `OnceLock` | Both run once under the hood with a lock |
| `ExceptionInInitializerError`, then `NoClassDefFoundError` on later access | `LazyLock` poisoning: "LazyLock instance has previously been poisoned" | The same design: a failed static initializer fails forever |
| Double-checked locking with `volatile` (broken before JSR-133, Java 5) | `OnceLock::get_or_init` | The right orderings are inside std (Part XIV) |
| `AtomicReference<Config>` swapped on refresh | `ArcSwap<Config>` | Java's GC frees the old config; Rust must reference-count it (debts) and free it *somewhere* |
| `ThreadLocal<T>` | `thread_local!` + `Cell`/`RefCell` | Rust's can't leak to other threads |
| `AtomicLong` / `LongAdder` | `AtomicU64` / per-thread counters merged on read (Chapter 11.7) | `LongAdder` is striped counters: Chapter 11.7's padded row |

> **Analogy limit.** `ArcSwap` looks like `AtomicReference`, and the reading side really is similar. The difference
> is at the other end of a value's life. In Java, the old config becomes garbage and the collector frees it later, on a
> GC thread. In Rust, the old config is freed **when its last `Arc` drops, on whichever thread drops it**, possibly a
> request thread in the middle of serving a request. Chapter 3.1's route-table incident was exactly that (a 1M-entry
> table freed on a request thread, a p99.9 spike). With `ArcSwap` you must decide where frees happen.

### 9. Production scenario

**Chapter 3.1's route table, now multi-threaded.** Meridian's gateway refreshes a 1M-entry route table every 10
minutes. With many worker threads reading it constantly, the design is `ArcSwap<RouteTable>` plus Chapter 3.1's dropper
thread, upgraded so the dropper waits until it holds the **last** reference (listing `ch04-04-arc-swap-routes.rs`,
excerpt):

```rust,ignore
// The dropper waits until it holds the LAST reference, so the free always happens here.
let (old_tx, old_rx) = mpsc::channel::<Arc<RouteTable>>();
let dropper = thread::Builder::new()
    .name("dropper".into())
    .spawn(move || {
        for mut old in old_rx {
            loop {
                match Arc::try_unwrap(old) {
                    Ok(t) => break drop(t),
                    Err(still_shared) => {
                        old = still_shared; // a reader still holds it: wait for them to finish
                        thread::yield_now();
                    }
                }
            }
        }
    })
    .unwrap();
```

```rust,ignore
// The writer: build each new table OFF to the side, publish with one atomic swap.
for v in 1..=50 {
    let fresh = Arc::new(RouteTable::build(v, N));
    let old = table.swap(fresh);
    old_tx.send(old).unwrap();
}
```

Four reader threads look up routes continuously while the writer publishes 50 versions. Every entry is stamped with its
table's version, so a torn read (entries from two versions) would show up as a mismatch. Verified in debug and release:

```text
lookups > 0: true
torn reads: 0
current version: 50, highest version a reader saw: 50
old tables freed on the dropper thread: 50, elsewhere: 0
```

Zero torn reads, because readers only ever see complete tables, and all 50 old tables were freed on the dropper thread,
because `try_unwrap` succeeds only for the last owner. This is also the answer to Chapter 3.3's design exercise (the
200 MB pricing catalog): **build the new catalog off to the side, publish it with one atomic swap, let in-flight quotes
finish on the old one, and free the old one somewhere that doesn't serve requests.**

### 10. Failure scenario

**The config that failed once and then forever.** A Meridian service read its payment-provider settings through a
`static PROVIDER: LazyLock<ProviderConfig>` that parsed environment variables on first access. After a deploy with one
variable misspelled, the first request that touched `PROVIDER` panicked inside the initializer. That request failed,
which was expected. What wasn't expected: **every later request also panicked**, with `LazyLock instance has previously
been poisoned`, exactly as the listing's `LazyLock access #2` line shows. The service stayed up (panics were caught per
request) and served 100% errors until it was restarted with the fixed variable.

Two things made it worse than a crash. Health checks passed, because they didn't touch `PROVIDER`. And the error rate
alert said "internal errors" without the root cause, since the original panic message appeared once, at the first
failure, hours before anyone looked.

The fixes follow the Chapter 8.1 rule, "malformed never defaults":

1. **Load configuration eagerly, in `main`, and fail fast.** A process that can't parse its configuration should never
   pass a readiness check. Parse into a `ProviderConfig`, return an error with context, and exit nonzero.
2. **Pass the config down** (`Arc<ProviderConfig>`) instead of reaching for a static.
3. **If lazy initialization is genuinely needed** (an expensive, optional resource), use `get_or_init` over a value
   that *records* the failure, such as a `Result`, so failure is a value callers handle, not a poison they trip over.
   (`OnceLock::get_or_try_init` would say this directly, but it is still unstable on 1.98.1: using it on stable is
   `error[E0658]`, "use of unstable library feature `once_cell_try`" (listing `ch04-08-try-init-unstable.rs`) [VERSION].)

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XI).*

1. Why is `UnsafeCell` the only legal way to mutate through `&T`? What three things does it change?
2. How do `Cell` and `RefCell` each re-establish aliasing XOR mutation? Why is `Cell` limited to `get` for `Copy`
   types?
3. How does `OnceLock::get_or_init` guarantee that the initializer runs once under a race? What does a read cost
   afterwards?
4. What happens when a `OnceLock` initializer panics? A `LazyLock` initializer? Why the difference?
5. Why is `OnceCell<String>` the same size as `String`, but `OnceCell<u64>` larger than `u64`?
6. Why is `parking_lot::Mutex<()>` 1 byte, when std's is 8? Where do its waiters go?
7. Why did `RwLock<Arc<T>>` reads cost more than `Mutex<Arc<T>>` reads in the measurement? Why is `ArcSwap::load`
   about 28× faster?
8. What problem do `arc-swap`'s "debts" solve?
9. With `ArcSwap`, which thread frees the old value? How do you control it?
10. When would you use `LazyLock` for configuration, and when not?

### 12. Exercises

- **Beginner.** Replace `ch04-06`'s `MyCell` with `std::cell::Cell` and add a `replace` method to `MyCell` with its own
  `SAFETY` comment. Run both under Miri.
- **Intermediate.** Build `MyOnceCell<T>` on `UnsafeCell<Option<T>>` with `get_or_init(&self, f) -> &T`. Write the
  `SAFETY` argument. Then find the reentrancy hole: what if `f` itself calls `get_or_init` on the same cell? (std's
  version panics. Why must it?)
- **Advanced.** Implement the listing's per-request folding with `thread_local!` per-worker counters instead: each
  worker adds to its own `Cell`s, and a reporter thread reads all of them. What does the reporter need, and why can't it
  read another thread's `Cell`?
- **Systems.** Extend `ch04-05-read-path-cost.rs` with 1, 2, 4, and 8 reader threads. Which designs degrade with thread
  count, and which stay flat? Explain each in cache-line terms.
- **Architecture.** Meridian's feature-flag service pushes updates every few seconds to 400 gateway workers. Design the
  in-process representation: `ArcSwap`, `RwLock`, or per-worker copies over a channel? Include where old versions are
  freed and how you'd detect a worker that stopped receiving updates.

### 13. Debugging exercise

A service caches tenant settings like this:

```rust,ignore
thread_local! {
    static SETTINGS: RefCell<HashMap<TenantId, Settings>> = RefCell::new(HashMap::new());
}

fn settings_for(t: TenantId) -> Settings {
    SETTINGS.with(|s| {
        let map = s.borrow();
        if let Some(v) = map.get(&t) {
            return v.clone();
        }
        let loaded = load_settings(t); // may call settings_for() for the parent tenant
        s.borrow_mut().insert(t, loaded.clone());
        loaded
    })
}
```

1. The first call for a child tenant panics with `already borrowed: BorrowMutError`. Which borrow is still alive at
   `borrow_mut`, and why?
2. There's a second problem when `load_settings` recurses. What is it?
3. Rewrite it so both are impossible. Would `OnceCell` per tenant, or a different structure, express the intent better?

### 14. Design exercise

**Meridian's pricing catalog (Chapter 3.3's exercise, now concurrent).** About 200 MB of prices and rules, read by 32
quote threads at 50K quotes/s, updated every few seconds from a feed. A quote must see one consistent catalog from start
to finish.

- Choose the publication mechanism and justify it against `RwLock<Catalog>` using the twelve criteria.
- How much memory do you budget for coexisting versions if the slowest quote takes 2 s and updates arrive every 3 s?
- Where do old catalogs get freed? What happens to p99 if they're freed on quote threads?
- How does a quote thread learn which catalog *version* it priced with, for the audit log?
