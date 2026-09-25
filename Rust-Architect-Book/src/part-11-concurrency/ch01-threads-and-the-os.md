# Chapter 11.1 — Threads and the OS

> **Where this sits:** Part XI · Concurrency · chapter 1 of 7
> **Prerequisites:** Chapter 1.3 (the first data race and its three fixes), Chapter 3.1 (ownership), Chapter 8.3
> (panics and unwinding), the Part IX interlude (stack size, guard pages).
> **After this chapter you can:** say exactly what `std::thread::spawn` asks the kernel for; read a thread's stack and
> guard page in `/proc`; explain why a `JoinHandle<T>` is an ownership transfer in reverse; predict what a panic in a
> thread does and doesn't take down; and size a thread pool from measurements instead of habit.

---

## Pass 1 · User level — *Moving work, and ownership, to another thread*

### 1. Problem

A Java engineer has used threads for years, mostly through an `ExecutorService`. Rust's `std::thread` looks familiar
at first, then raises questions quickly. Why does `spawn` insist on `'static`? Why does the process exit while a thread
is still writing? What does a thread cost, and when is "one more thread" the wrong answer?

This chapter answers those questions from the bottom up, starting with what the kernel sees. The rest of Part XI builds
on it: the type rules that make sharing safe (11.2), the locks (11.3), interior mutability (11.4), channels (11.5),
scoped and data-parallel threads (11.6), and the decision matrix (11.7). The two projects then assemble all of it into an
HTTP server and the first version of **Ferrite**.

### 2. Mental model

A `std::thread` is an **operating-system thread, one to one** [LIB] [OS]. There's no green-thread scheduler inside
`std`: Rust removed its runtime before 1.0 (Chapter 1.2). Everything else follows from treating a thread as a
**destination that ownership travels to and a result that ownership travels back from**:

```text
 parent thread                                        new OS thread
 ─────────────                                        ─────────────
 let h = thread::spawn(closure) ───── moves ────────► runs closure()          closure: FnOnce() -> T + Send + 'static
        │                             (captured values                 │
        │                              now owned there)                ├─ returns T    ─┐
        │                                                              └─ panics (P)   ─┤ stored in a shared slot
 h.join() ◄──────────────────────── moves back ─────────────────────────────────────────┘
   = Ok(T)  or  Err(Box<dyn Any + Send>)  (the panic payload P)

 drop(h) without join()  →  the thread is DETACHED: it runs on, and nobody waits for it
 main returns            →  the PROCESS exits: every other thread stops mid-instruction
```

Three consequences:

1. **`'static` on `spawn` is an ownership statement, not a lifetime puzzle** [LANG]. The new thread can outlive the
   function that spawned it (that's what "detached" means), so it can't borrow that function's locals. It must **own**
   what it uses. Chapter 11.6 covers the alternative, scoped threads, which *can* borrow because they provably end
   first.
2. **A thread's result is a value you own.** `JoinHandle<T>` is a claim ticket for `T`. No shared variable, no latch,
   no `Future.get()` with checked exceptions.
3. **`main` is special.** When it returns, the process exits. The JVM waits for non-daemon threads. Rust doesn't
   (verified in §10).

### 3. Rust code

Spawning, naming, choosing a stack size, and joining (listing `ch01-01-threads-are-tasks.rs`, excerpt):

```rust,ignore
for i in 0..n {
    let (ready, done) = (Arc::clone(&ready), Arc::clone(&done));
    let builder = thread::Builder::new().name(format!("worker-{i}"));
    // One worker asks for a smaller stack, to show that the size is just an mmap length.
    let builder = if i == 3 { builder.stack_size(256 * 1024) } else { builder };
    handles.push(
        builder
            .spawn(move || {
                let local = 0u8; // lives on this thread's stack
                let (mapping, guard) = stack_mapping(&local as *const u8 as usize);
                let tid = gettid();
                ready.wait(); // everyone alive at the same time
                done.wait(); // main has looked at /proc; now exit
                (thread::current().name().unwrap().to_string(), tid, mapping, guard)
            })
            .expect("spawn failed"),
    );
}
```

`thread::spawn(f)` is `Builder::new().spawn(f).unwrap()`. Use `Builder` in services. `spawn` returns
`io::Result<JoinHandle<T>>`, and thread creation *can* fail: the process may have hit its thread limit, or the kernel may
refuse the memory. A server should turn that into backpressure, not a panic.

**Panics stay in their thread.** The payload is handed to whoever joins (listing `ch01-02-join-panic.rs`):

```rust
use std::any::Any;
use std::thread;

fn describe(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        format!("&str payload: {s:?}")
    } else if let Some(s) = payload.downcast_ref::<String>() {
        format!("String payload: {s:?}")
    } else {
        "non-string payload".to_string()
    }
}

fn main() {
    // Quiet the default hook so stdout shows just the program's view (the hook would print to stderr).
    std::panic::set_hook(Box::new(|_| {}));

    let ok = thread::spawn(|| 6 * 7);
    let literal = thread::spawn(|| -> u32 { panic!("bad input") });
    let formatted = thread::spawn(|| -> u32 {
        let shard = 3;
        panic!("shard {shard} corrupted")
    });

    println!("ok:        {:?}", ok.join());
    match literal.join() {
        Ok(v) => println!("literal:   {v}"),
        Err(payload) => println!("literal:   Err({})", describe(&*payload)),
    }
    match formatted.join() {
        Ok(v) => println!("formatted: {v}"),
        Err(payload) => println!("formatted: Err({})", describe(&*payload)),
    }

    // A scope joins every thread it spawned; if one panicked and nobody joined it explicitly,
    // the scope itself panics when it ends.
    let r = std::panic::catch_unwind(|| {
        thread::scope(|s| {
            s.spawn(|| panic!("inside scope"));
        });
    });
    println!("scope with an unjoined panicking thread: {}", if r.is_err() { "scope panicked" } else { "ok" });
    println!("main is still running");
}
```

```text
ok:        Ok(42)
literal:   Err(&str payload: "bad input")
formatted: Err(String payload: "shard 3 corrupted")
scope with an unjoined panicking thread: scope panicked
main is still running
```

The payload type depends on how the panic was written, as Chapter 8.3 showed: a literal message is a `&'static str`,
and a formatted one is a `String`. Code that logs thread failures must try both.

---

## Pass 2 · Systems level — *What the kernel sees*

### 4. Under the hood

**What `spawn` does** [LIB] (std's current implementation on Linux; the shape, not a contract):

```text
Builder::spawn(f)
  ├─ Box the closure together with an Arc<Packet<T>>      (Packet = the slot where the result will land)
  ├─ pthread_create(attr: stack size = 2 MiB default, or Builder::stack_size / RUST_MIN_STACK)
  │     └─ glibc: mmap the stack + a PROT_NONE guard page, then clone3/clone with CLONE_VM | CLONE_THREAD | ...
  │                                                      (same address space, new kernel task)
  └─ return JoinHandle { native pthread_t, Arc<Packet<T>>, Thread handle }

new thread's entry
  ├─ register thread info (name, id); set up thread-local storage
  ├─ run f() inside catch_unwind          → Ok(T) or Err(payload)
  └─ store the result in the Packet; drop captured state; exit (pthread exit, stack unmapped by glibc)

JoinHandle::join()
  └─ pthread_join  →  take the result out of the Packet  →  Result<T, Box<dyn Any + Send>>
```

"Kernel task" is the key phrase. On Linux, a thread *is* a task that shares its address space with others (the flags
`CLONE_VM | CLONE_THREAD` say so) [OS]. You can see this from inside the program. While four workers wait at a barrier,
`main` lists `/proc/self/task`, and each worker reports its TID and the memory mapping that holds its stack (listing
`ch01-01-threads-are-tasks.rs`, rustc 1.98.1 on the Playground):

```text
main: pid 13, tid 13
while running: /proc/self/task = ["13", "43", "44", "45", "46"], Threads: 5
worker-0: tid 43, stack 741d31e42000-741d32042000 rw-p 2048 KiB
           guard 741d31e41000-741d31e42000 ---p
worker-1: tid 44, stack 741d31c41000-741d31e41000 rw-p 2048 KiB
           guard 741d31c40000-741d31c41000 ---p
worker-2: tid 45, stack 741d31a3a000-741d31c3a000 rw-p 2048 KiB
           guard 741d31a39000-741d31a3a000 ---p
worker-3: tid 46, stack 741d319f9000-741d31a39000 rw-p 256 KiB
           guard 741d319f8000-741d319f9000 ---p
after join:    Threads: 1
```

Read it line by line:

- **The main thread's TID equals the PID.** The first task of a process is its thread-group leader. Each spawned
  thread is a new task with its own TID (43–46), listed under `/proc/self/task`.
- **Each stack is a 2048 KiB anonymous mapping** (`rw-p`), exactly Rust's default [LIB]. It is *not* glibc's default for
  `pthread_create`, which follows `ulimit -s`, typically 8 MiB. Rust passes its own size. `stack_size(256 * 1024)`
  produced a 256 KiB mapping: the size is just an `mmap` length.
- **Directly below each stack sits a 4 KiB `---p` mapping: the guard page.** Stacks grow down, so running off the end
  touches a page with no permissions. That fault is what the Part IX interlude turned into the clean
  `has overflowed its stack` abort.
- **After the joins, the process has one thread again.** Joined threads are gone, and their stacks are unmapped.

To see the actual system calls, run the listing locally under `strace -f -e trace=clone,clone3,mmap,munmap`. (Not
verified here: the Playground has no `strace`.)

**`join` is the only synchronization you get for free.** When `join` returns, everything the thread did
**happens-before** the code after the join [LANG]. That's why Chapter 1.3's counter could use `Ordering::Relaxed` and
still read the right total after joining. Part XIV defines happens-before properly.

### 5. Memory

A thread's memory has three parts, and they're easy to confuse:

| Part | Size | Committed when? |
|---|---|---|
| Stack mapping (virtual) | 2 MiB default [LIB] + 4 KiB guard | Reserved at spawn; pages become resident only when touched |
| Stack pages actually used (RSS) | usually a few to tens of KiB | On first touch (page fault) [OS] |
| Kernel side: task struct, kernel stack | order of 10–20 KiB per thread on x86-64 Linux (order of magnitude) [OS] | At clone |
| Thread-local storage, the `Packet`, the boxed closure | small, per thread | At spawn |

So "10,000 threads × 2 MiB = 20 GB" is **virtual** address space, which a 64-bit process has plenty of. What actually
limits you is resident memory for touched stack pages, the kernel's per-thread cost, and the limits: `ulimit -u`,
`/proc/sys/kernel/threads-max`, cgroup `pids.max` in containers, and `vm.max_map_count`, since each thread adds two
mappings [OS]. Chapter 19.5 goes deeper into virtual memory.

Two Rust-specific notes. First, **the stack size is a deployment parameter**, not a constant. Recursion depth, big
stack arrays (Chapter 2.4's 4 MiB export-service crash), and deep async state machines all compete for it. Set it
explicitly with `Builder::stack_size` for threads you create. `RUST_MIN_STACK` changes the default for threads you
don't. Second, **the main thread's stack comes from the OS loader** (8 MiB on typical Linux, 1 MiB on Windows, from the
Part IX interlude), not from Rust's 2 MiB default. Code that works in `main` can overflow on a worker.

### 6. CPU / OS

**Creating a thread costs microseconds, and handing work to an existing thread costs a few.** One run on the
Playground (release, listing `ch01-03-spawn-cost.rs`; noisy):

```text
spawn + join (std::thread::spawn):    33.76µs per thread
spawn + join (thread::scope):         27.93µs per thread
hand-off to existing worker:           5.67µs per round trip
```

The spawn cost is `mmap` + `clone` + scheduling + `munmap` + the join handshake. The hand-off is two channel
operations with two wakeups: futex calls plus two context switches. That's roughly a 6× difference per task, and
creating threads also produces allocator and kernel churn that the numbers don't show. **Threads are for long-lived
workers, not per-request objects.**

**More threads than cores doesn't mean more throughput.** The kernel time-slices runnable threads. For CPU-bound work,
extra threads only add context switches and **delay when each task finishes**. Here are 64 jobs of about 5 ms of pure
CPU each on the Playground's 4 cores (listing `ch01-04-oversubscription.rs`, one run):

```text
cores = 4, 64 jobs of ~5 ms CPU each
thread per job (64):   total 115.0ms   first job done   6.5ms   mean completion  73.1ms   last 114.8ms
pool of 4:             total 108.0ms   first job done   5.4ms   mean completion  57.1ms   last 108.0ms
```

Total time is about the same, since the CPU work is fixed, but **mean completion time is 28% worse** with a thread per
job. The scheduler interleaves all 64, so most finish near the end. The pool runs them to completion in order, so early
jobs finish early. In latency terms, that's the difference between a p50 near the p100 and a p50 near the middle. It's
Little's law from the other side: for the same throughput, more work in progress means longer time in the system.

[OS] Linux's scheduler (CFS, replaced by EEVDF in kernel 6.6 [VERSION]) aims for fairness among runnable tasks, which
is exactly the wrong goal for latency-sensitive CPU work competing with itself. That's why the standard designs are:

- **A pool sized to the cores** for CPU-bound work (Chapter 11.6's rayon, sized to logical CPUs by default [LIB]).
- **A larger pool for blocking I/O**, sized by Little's law: `threads ≈ throughput × time blocked per task`.
- **Thread-per-core with pinned threads and no shared state** for the most latency-sensitive systems (some databases
  and proxies). It's a design that Part XIII's async runtimes and Part XX's NUMA discussion come back to.

---

## Pass 3 · Architect level — *How many threads, and who owns them?*

### 7. Trade-offs

**Thread per task vs a fixed pool vs async tasks** (the SPEC's twelve criteria; async is covered in Parts XII–XIII
and listed here for orientation only):

| Criterion | Thread per task | Fixed pool + bounded queue | Async tasks (Part XII) |
|---|---|---|---|
| **Memory** | 2 MiB virtual + touched pages + kernel state per task | Fixed: N stacks | Bytes to KiB per task (the future's size) |
| **CPU** | ~30 µs create/destroy per task (measured) | ~5 µs hand-off (measured) | Sub-microsecond task switch (order of magnitude) |
| **Latency** | Good at low load, erratic under oversubscription | Queueing delay at saturation, bounded and visible | Good while tasks don't block the executor |
| **Throughput** | Collapses when threads ≫ cores (switching, memory) | Stable at saturation | Highest for I/O-bound work |
| **Contention** | Unbounded threads contending for shared locks | Bounded by pool size | Same, plus no blocking allowed on executor threads |
| **Cache behavior** | Threads migrate and evict each other's working sets | Stable per-worker working sets | Tasks migrate between workers (work stealing) |
| **Allocation** | Stack mmap + packet per task | A queue slot (or a box) per job | A task allocation per spawn |
| **Complexity** | Trivial code | Moderate: queue, shutdown, rejection policy | Higher: runtime, `Send` futures, cancellation |
| **Safety** | Same type rules everywhere (11.2) | Same | Same, plus "don't block" is unchecked |
| **Maintainability** | Easy to read, hard to operate | Explicit capacity in the code | Pervasive `async` coloring |
| **Failure modes** | Thread exhaustion, OOM under load spikes | 503/backpressure at saturation (by design) | Executor starvation by blocking calls |
| **Operational implications** | Load spikes become thread spikes | Pool size and queue depth are tunables and metrics | Runtime metrics (task counts, poll times) |

The rule: **a thread is a long-lived resource you budget, like a database connection.** Create a fixed number, give
each one a job loop, and make admission (the queue bound) the explicit control point. Project L3 builds exactly that.

### 8. Java comparison

| Java | Rust | Note |
|---|---|---|
| `new Thread(r).start()` | `thread::spawn(f)` / `Builder::spawn` | Both are 1:1 OS threads (Java *platform* threads) |
| `thread.join()` + shared field for the result | `handle.join()` → `Result<T, payload>` | Rust's result travels by ownership |
| Uncaught exception handler | Panic hook + `join()`'s `Err` | Neither kills the process by default (unless `panic = "abort"`) |
| `Thread.interrupt()` | *No equivalent* | Cancellation is cooperative: flags, channels, closing a queue |
| Daemon vs non-daemon threads; JVM exits when non-daemon threads finish | Process exits when `main` returns | **Opposite default** (§10) |
| `-Xss` (default 1 MiB on 64-bit Linux, per the JDK docs) | 2 MiB default, `Builder::stack_size`, `RUST_MIN_STACK` | Both are virtual reservations |
| `ExecutorService` (`newFixedThreadPool`) | A pool you choose: Project L3, rayon, Tokio | `newFixedThreadPool`'s queue is **unbounded** by default (Chapter 11.5) |
| Virtual threads (JDK 21, JEP 444): M:N, cheap blocking | Async tasks (Parts XII–XIII) | Different mechanism: stackful vs stackless (Chapter 12.6) |
| `ThreadLocal<T>` | `thread_local!` | Rust's version can't be reached from another thread at all |

> **Analogy limit.** "A Rust thread is a Java platform thread" is true about the kernel and false about the program.
> A Java thread can reach any object reachable from a static field or the `Runnable`'s captures, and the type system
> doesn't care whether those objects are thread-safe. A Rust thread can reach **only what it owns or what the type
> system proved shareable** (`Send`/`Sync`, next chapter). The kernel object is the same. The set of reachable memory is
> not.

Interruption deserves a second look. Java's `interrupt()` is a request that blocking methods notice by throwing
`InterruptedException`. Rust has nothing like it, deliberately: there's no safe way to stop a thread at an arbitrary
point without leaking locks or leaving data half-updated. Every cancellation in Rust is **cooperative and explicit**: a
flag the worker checks, a closed channel, a closed queue (Project L3's shutdown). Async Rust's cancellation, dropping a
future at an `.await` (Chapter 13.4), is the one place that looks like forced cancellation, and it has its own rules.

### 9. Production scenario

**Meridian's webhook dispatcher.** Partners register webhooks for payment events. The Java dispatcher created one
platform thread per outbound call, so a slow partner simply meant more threads. During a partner's 40-minute outage, calls
hung until their 30-second timeout, threads piled up to about 28,000, and the JVM failed with
`OutOfMemoryError: unable to create native thread`. It took the *healthy* partners' deliveries down with it.

The Rust rewrite made threads a budget:

- **A fixed pool per partner tier** (`Builder::new().name("webhook-gold-3")...`): named threads show up in `top -H`,
  in `/proc/<pid>/task/*/comm`, and in panic messages.
- **A bounded queue per partner.** When a partner's queue is full, new events go to a durable retry table instead of
  spawning more work. A slow partner consumes *its* queue, not the process.
- **`Builder::spawn` errors are handled.** Startup fails loudly if the pool can't be created, instead of limping along.
- **An explicit stack size for workers** (`stack_size(512 * 1024)`, measured with the Part IX interlude's frame-size
  technique plus a 4× margin), because the payload renderer recursed over nested JSON.

The outage is now a queue-depth graph for one partner and a retry backlog, with no effect on the others. The pool size
became a line in the capacity plan: `threads = peak calls/s × p99 call time`, from Little's law.

### 10. Failure scenario

**The settlement upload that never happened.** Meridian's settlement batch job (Rust, Chapter 8.1) spawned a
background thread to upload the finished batch file, then returned from `main`. In Java, the JVM would have waited for
the non-daemon thread. In Rust, it doesn't (listing `ch01-05-main-exits.rs`):

```rust
use std::thread;
use std::time::Duration;

fn main() {
    let _uploader = thread::spawn(|| {
        thread::sleep(Duration::from_millis(100)); // "upload the last batch file"
        println!("uploader: batch uploaded"); // never printed: the process is gone by then
    }); // the JoinHandle is dropped: the thread is detached, and nobody waits for it
    println!("main: done, returning");
}
```

```text
main: done, returning
```

No error, no panic, no log line: the upload never ran. The file sat in the outbox until the partner's reconciliation
flagged a missing batch the next morning.

The fix has three layers. **Join what you spawn**: keep the `JoinHandle` and `join()` it before returning, or use
`thread::scope`, which can't return early. **Make shutdown a code path**: a `Drop` that closes the work queue and joins
the workers, as Project L3's pool does. And **treat "the process is exiting" as a state the program handles**, not an
event it assumes won't happen. `let _uploader = ...` is the same smell as Chapter 1.3's `let _ = lock()`: an underscore
binding that throws away the one handle that mattered.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XI).*

1. What does `std::thread::spawn` create at the OS level on Linux? Where does the 2 MiB come from, and what is the
   `---p` mapping below each stack?
2. Why does `thread::spawn` require `F: 'static`? What would go wrong without it?
3. What does `JoinHandle::join` return, and why is it a `Result`? What are the possible payload types?
4. What happens to other threads when `main` returns? How does that differ from the JVM, and what design rule follows?
5. Why is a thread's 2 MiB stack not 2 MiB of memory? What actually limits the number of threads on a Linux host?
6. Creating a thread took about 30 µs and a hand-off about 5 µs in this chapter's run. What does each consist of?
7. Why did 64 threads on 4 cores have a worse *mean completion time* than a 4-thread pool, with the same total time?
8. How do you size a pool for CPU-bound work? For blocking I/O? Name the law.
9. Why doesn't Rust have `Thread.interrupt()`? How do you cancel a thread?
10. When would you choose thread-per-core over a shared pool?

### 12. Exercises

- **Beginner.** Spawn eight named threads that each return the sum of a different range of numbers. Join them, add the
  results, and print which thread computed what. Then make one thread panic and report which one failed and why,
  without crashing `main`.
- **Intermediate.** Extend `ch01-01-threads-are-tasks.rs` to read each worker's `/proc/self/task/<tid>/stat` and print
  its CPU number (field 39) and state. Run it a few times. Do threads migrate between CPUs?
- **Advanced.** Write a `spawn_with_retry(builder, f, attempts)` that turns `Builder::spawn` errors into bounded
  retries with backoff, then a final error. Where would you *not* want this?
- **Systems.** Locally, run `ch01-03-spawn-cost.rs` under `strace -f -c` and `perf stat -e context-switches`. How many
  system calls does one `spawn + join` make? One hand-off? Compare with the Playground numbers.
- **Architecture.** Meridian's dispatcher has three partner tiers with different call rates and p99 latencies (gold:
  200 calls/s at 300 ms; silver: 50 calls/s at 1 s; bronze: 5 calls/s at 10 s). Size each pool with Little's law, add
  headroom, and design what happens when one tier saturates.

### 13. Debugging exercise

A CLI tool starts a thread per input file and prints each file's line count. Sometimes the output is missing lines,
sometimes it's complete:

```rust,ignore
fn main() {
    for path in std::env::args().skip(1) {
        std::thread::spawn(move || {
            let n = std::fs::read_to_string(&path).map(|s| s.lines().count()).unwrap_or(0);
            println!("{path}: {n}");
        });
    }
}
```

1. Explain the nondeterminism in terms of §2's model. Which thread's exit decides what gets printed?
2. Fix it two ways: with `JoinHandle`s, and with `thread::scope`. Which one also lets the threads borrow a shared
   `&Config`?
3. The tool is later run on 50,000 files. What breaks, and how would you restructure it (Chapter 11.6)?

### 14. Design exercise

**A report renderer for Meridian's merchant portal.** Requests arrive at up to 300/s. Each report spends about 40 ms
querying the ledger (blocking I/O) and 15 ms of CPU formatting a PDF, and the service runs on 8 cores. Design the
threading:

- one pool or two (I/O and CPU)? What are their sizes (show the Little's law arithmetic)?
- where are the queues, and what bounds them? What does a client see when they're full?
- what stack size do the formatting threads need, and how will you *measure* it rather than guess?
- how does the service shut down during a deploy without dropping an in-progress report?

Then fill in the twelve-row matrix from §7 for your design against "thread per request".
