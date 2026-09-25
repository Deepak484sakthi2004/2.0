# Part XII — Async Rust

> **Part question:** *When you write `async fn foo() {}` and `.await` it, what exactly does the compiler build, who runs
> it, how does it wait without a thread, and when is that design the right one for a system?*

Part XI gave you threads: real, kernel-scheduled, each with its own stack. They're the right tool for most concurrency,
and they stop scaling somewhere in the thousands. This Part is about the other tool. It starts where async started, with
a server holding tens of thousands of mostly idle connections, and it doesn't treat `async`/`await` as magic at any
point. You'll poll futures by hand, read the state machine rustc generates (in real MIR and real assembly), break `Pin`
under Miri to see what it protects, and build an executor, a timer, and an epoll reactor from scratch, about 150 lines
each.

The pipeline the Part follows, one chapter per stage:

```text
 async fn / async block                              12.3  what the compiler generates
        │  calling it runs nothing: it returns a value
        ▼
 Future  (a state machine: one variant per .await)   12.2  the trait and its contract
        │  someone must call poll()                   12.4  why that value must not move: Pin
        ▼
 poll(self: Pin<&mut Self>, cx) → Pending | Ready
        │  Pending = "I have arranged to be woken"
        ▼
 Waker  (data pointer + vtable, 16 bytes)
        │  wake() puts the task back in a run queue
        ▼
 executor + reactor + timer                           12.5  built from scratch
        │  when nothing is runnable: epoll_wait / park
        ▼
 the OS: sockets, readiness, threads                  12.1  why any of this exists
                                                      12.6  threads vs green threads vs tasks
```

## Chapter map

```text
12.1 Why Async Exists            thread-per-connection hits a wall at connection 503 on the Playground (verified);
                                 nonblocking I/O, epoll by hand (LT vs ET, run under Miri), 10,000 connections
                                 on one mio thread; Rust's green threads and why they were removed (RFC 230)
      │
12.2 The Future Trait and poll   lazy futures, the Pending obligation, lost wakeups that hang forever (verified),
                                 join/select by hand, cancellation = drop, CompletableFuture compared
      │
12.3 async fn → State Machine    the coroutine layout in real MIR, the jump table in real asm, future sizes,
                                 why futures become !Send, async recursion, async fn in traits, async closures
      │
12.4 Pin                         a self-referential struct dangling under Miri, the same struct pinned (Miri-clean
                                 under Stacked and Tree Borrows), pin projection, and one safe `impl Unpin` that
                                 turns it into a use-after-free
      │
12.5 Wakers and Executors        block_on, a RawWaker by hand, a run queue + timer + JoinHandle, an epoll reactor
                                 serving 100 clients on one thread, a deterministic simulation executor
      │
12.6 Threads vs Green vs Async   memory per unit and switch cost measured; Go, Java virtual threads, Erlang;
                                 blocking in async, function coloring, and a decision matrix
      │
Part XII Review                  the merchant-notify PR: a dozen async defects, and a tested fix
```

## The running system

Most of this Part's scenarios come from **merchant-notify**, a new Meridian system. It pushes payment, refund, and
payout events to merchant dashboards and point-of-sale terminals over long-lived connections: about 600,000 concurrent
connections at peak, more than 95% of them idle at any moment. It's the classic workload async was invented for.
payments-core (Part VIII) and the marketplace payouts service supply the rest.

## What you'll be able to do after Part XII

- Explain, at the level of MIR and assembly, what `async fn` compiles to and what a `.await` costs.
- Predict a future's size, its `Send`-ness, and what dropping it mid-flight does to your invariants.
- Write a correct hand-written `Future`, including the waker obligations that no compiler checks.
- Explain what `Pin` guarantees, why `Unpin` exists, and write (or reject) an `unsafe` pin projection.
- Build an executor, and judge the design choices inside Tokio (Part XIII) as trade-offs, not magic.
- Choose between threads, Java virtual threads, goroutines, and Rust async for a given system, with numbers.

## Listings

`listings/part-12/`: 59 files, 75 checks, all verified on rustc 1.98.1 (edition 2024), including 12 runs under
**Miri** (three of them catching undefined behavior on purpose, two under Tree Borrows), 2 stack-overflow crashes, 2
expected panics, 13 intended compile errors, and a 5-test capstone. Crates used: `futures`, `mio`, `slab`, `libc`,
`pin-project-lite`, and `tokio` where a comparison needs a production runtime.
