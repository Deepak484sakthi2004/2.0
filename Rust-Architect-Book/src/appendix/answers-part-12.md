# Appendix A — Answer Key: Part XII

> Model answers. Write yours first. Where several answers are defensible, the key says so.

---

## Chapter 12.1 — Why Async Exists: From C10K to C10M

### Interview & architecture questions

**1. What a blocked `read(2)` costs.** One OS thread per waiting connection: a kernel task (scheduler entry, kernel
stack, task structure), a user stack (2 MiB reserved by default for a Rust-spawned thread, about 10 KiB actually
resident when idle), and a slot against every per-process and per-container task limit. The thread's stack is "where
the connection's state lives" because the blocked function's frames (which function it's in, its locals, what it's
waiting for) *are* the protocol state. Nothing else needs to remember where the connection is; that's why blocking code
is easy to write, and why every waiting connection costs a whole thread.

**2. 3 GB virtual, 7 MB resident.** Each handler thread reserves a 2 MiB stack with `mmap` (virtual, untouched), and
touches only a few KiB of it: 500 threads × ≈2 MiB ≈ 1 GB of reservations, but about 9.5 KiB resident each (7,128 −
3,324 KiB over 400 threads). The jump in the first 100 threads (2.1 GB, not 200 MB) is glibc's per-thread malloc
arenas: each reserves 64 MiB of address space on 64-bit, up to 8 × cores of them (32 on the 4-core sandbox), so about
30 × 64 MiB appeared as the first threads allocated [LIB]. Address space isn't RAM, which is why the process was killed by
a task limit, not by memory.

**3. select, poll, epoll.** `select` passes a bitmap of interest every call, costs O(highest descriptor), and is capped
by `FD_SETSIZE` (usually 1,024). `poll` passes an array every call: no cap, still O(n) per call and copied in each time.
`epoll` keeps the interest set in the kernel (`epoll_ctl` adds and removes). When a socket becomes ready, the kernel's
wakeup path puts it on the epoll instance's ready list, so `epoll_wait` only drains that list: its cost is proportional
to ready descriptors, not registered ones [OS]. With 10,000 connections and 20 active, `select`/`poll` walk 10,000;
`epoll_wait` returns 20.

**4. Level- vs edge-triggered.** Level-triggered reports a descriptor on every `epoll_wait` while it's ready (data
remains). Edge-triggered reports it once per transition from not-ready to ready. Verified in `ch01-03`: read 4 of 8
bytes and level-triggered reports it again; edge-triggered reports nothing. If you use edge-triggered mode (as mio does)
and read only part of the data, the rest sits in the socket buffer with no further event until the peer sends more:
the connection hangs. Rule: drain until `WouldBlock`.

**5. Readiness vs completion; owned buffers.** Readiness (epoll, kqueue) says "you may now read"; your code then reads
into its own buffer, and nothing is lent to the kernel between calls. Completion (io_uring, IOCP) says "read into this
buffer and tell me when it's done"; the kernel owns the buffer while the operation is in flight. In Rust, dropping a
future cancels it. If the future lent the kernel a `&mut [u8]` and was dropped mid-operation, the kernel could still
write into memory that's been freed or reused. So completion-based runtimes take the buffer by value and give it back
with the result: `read(buf: Vec<u8>) -> (io::Result<usize>, Vec<u8>)`.

**6. Kernel limits before RAM.** `pids.max` in the cgroup (`EAGAIN` from `clone`: "Resource temporarily unavailable",
exactly `ch01-01`'s failure at connection 503); `vm.max_map_count` (65,530 mappings, two per thread stack: about 32,000
threads, then `clone` fails, which was the 2019 incident's real cause); `kernel.threads-max` / `pid_max` and
`RLIMIT_NPROC` (`EAGAIN`); and `RLIMIT_NOFILE` (`EMFILE`), which applies to event loops too. In Java, all of these
surface as `OutOfMemoryError: unable to create native thread`, with a healthy heap.

**7. Why Rust removed green threads.** Embeddability. A mandatory runtime (M:N scheduler, segmented stacks, its own
I/O) can't be dropped into a C program, a kernel, a browser engine, or a microcontroller, and it taxes every program
whether it uses it or not. RFC 230 (2014) removed it: `std::thread` became plain OS threads. What replaced it, five
years later, was a *language* feature with no runtime: the `Future` trait in `core`, `async`/`await` compiled to state
machines, and executors, reactors, and timers as libraries.

**8. When thread-per-connection wins.** When connections are few (hundreds) or busy and requests are CPU-heavy: an
internal image-rendering or report-generation service, a database with a process or thread per session (PostgreSQL),
an admin API. Threads give preemption (a slow request can't starve the others), ordinary stack traces, and simple code,
and they can use blocking libraries (JDBC-style drivers, C libraries) without ceremony. Async buys nothing when the
work isn't waiting.

### Debugging exercise (the mio echo server)

1. mio registers sockets **edge-triggered** [LIB]. The code reads once into a 64-byte buffer. When a client sends more
   than 64 bytes, the rest stays in the socket buffer, and no new readiness event arrives until the client sends
   *more*, which it won't, because it's waiting for its echo. The fix is to read in a loop until `WouldBlock`.
2. Fixed read path: `loop { match stream.read(&mut buf) { Ok(0) => { close and deregister; break } Ok(n) => queue
   buf[..n] for writing, Err(e) if WouldBlock => break, Err(e) => { close; break } } }`. Note `Ok(0)` also needs
   handling: the original code ignores end-of-file, so a closed connection keeps its slab slot and descriptor forever.
   The **second bug** is `write_all` on a non-blocking socket. When the socket's send buffer is full, `write` returns
   `WouldBlock`, and `write_all` treats that as an error (it retries only on `Interrupted`). The `?` then returns that
   error from the event loop: one slow client stops the whole server. Bytes already written stay written, and the rest
   is lost. The fix is a per-connection output buffer: write what the socket accepts, keep the rest, register
   `WRITABLE` interest, flush when the socket becomes writable, and deregister writable interest when the buffer is
   empty (staying registered is the Advanced exercise's busy loop).
3. With a blocking thread per connection, `read` blocks until data is available and returns what's there. The next
   `read` returns the remainder immediately; there's no readiness *edge* to miss. And a blocking `write_all` blocks until
   everything is sent, so it can't fail with `WouldBlock`. The event loop has to reimplement both behaviors explicitly.

### Selected exercises

- **Beginner (`set_read_timeout`).** With a blocking socket and a read timeout, the thread makes **one** `read` call.
  The kernel wakes it the moment data arrives, so for data 20 ms away the latency is the same as a plain blocking read.
  The timeout only decides how often the call returns early with `WouldBlock`/`TimedOut` when nothing arrives (typically
  so the thread can check a shutdown flag). Too short a timeout burns system calls and wakeups; too long delays shutdown
  and failure detection. It doesn't add latency to data.
- **Advanced (`EPOLLOUT`).** A socket with room in its send buffer is almost always writable. Registered for `EPOLLOUT`
  in level-triggered mode, it's reported on every `epoll_wait`, so the loop never sleeps: 100% of a core spent
  discovering, over and over, that there's nothing to write. Register for writability only while output is pending (or
  use edge-triggered mode and track the state yourself).
- **Architecture (2 million SSE subscribers).** One defensible plan: 100,000 connections per pod, so 20 pods plus
  headroom for a zone failure (about 26). Per connection: an async task of a few hundred bytes plus a small heap buffer
  that grows only for large events; kernel socket memory that's small while idle but bounded by `tcp_wmem` under load,
  so size send buffers explicitly. Descriptors: `RLIMIT_NOFILE` above the per-pod limit plus margin. Heartbeats: at a
  30 s interval, 2 million connections generate about 67,000 frames per second fleet-wide, which is cheap but not free.
  Admission: a per-pod hard limit that rejects with a retry hint and jitter; per-API-key connection caps; idle reaping;
  and a reconnect budget so that a regional blip doesn't become a storm.

---

## Chapter 12.2 — The Future Trait and poll

### Interview & architecture questions

**1. Laziness.** A future is a value; the work happens inside `poll`, and creating the value calls nothing. If futures
were eager, creating one would have to start the work somewhere: on a thread, or on a runtime known at creation time.
That needs a runtime at every call site, allocations for detached work, and it breaks cancellation-by-drop (the work is
already running elsewhere). It also breaks zero-cost composition: `join` and `select` can be plain struct fields only
because nothing runs until the parent polls them.

**2. The `Pending` obligation.** Returning `Pending` means "I have arranged for the waker in **this** `cx` to be woken
when polling me again can make progress." If a future returns `Pending` without doing that, nothing will ever poll it
again. There's no error because nothing is watching: no thread is blocked, the executor has no timeout for a task that
isn't in its queue, and the future just sits in memory. `ch02-04` shows the event happening (`done = true`) and the
task hanging anyway.

**3. Why the waker comes with every poll.** A future can move between tasks and executors during its life: created and
first polled inside one task's `select!`, then moved into a spawned task; polled by `block_on` in a test and by Tokio in
production. The waker that must be woken is the one for whoever polled it *last*. The merchant-notify `AckWait`
incident (12.2 §10) is exactly this: the first poll happened in the connection task, the ACK woke the connection task,
and the delivery task that owned the future waited 30 seconds.

**4. `Waker::will_wake`.** It compares the two wakers' data pointers and vtables, and returns true when they're
identical, meaning waking either would wake the same task. It can return false for wakers that would in fact wake the
same task (it's a cheap check, not a proof). It's useful for skipping the clone (a refcount increment, sometimes more)
on the common path where the same task polls again: `Some(w) if w.will_wake(cx.waker()) => {}`.

**5. Cancellation = drop.** Dropping a future that's suspended runs the destructors of exactly the values saved in its
current state (its drop glue switches on the state tag), and nothing after the current `.await` ever runs. What
*doesn't* happen: anything the future started outside itself (a thread, a spawned task, a request already on the wire, a
database transaction) keeps going unless some value's `Drop` stops or undoes it.

**6. `join(30 ms, 20 ms)` takes 30 ms.** `join` polls both children in the same task. On the first poll both start
their timers and return `Pending`; both waits then overlap, and the join finishes when the slower one does. Concurrency
without parallelism: one thread, two outstanding waits.

**7. `cancel(true)` vs drop.** `CompletableFuture.cancel(true)` completes the future exceptionally with a
`CancellationException`, and dependents see that, but the computation producing the value **isn't interrupted** (for
`CompletableFuture` the `mayInterruptIfRunning` flag has no effect). The charge still happens and its result is thrown
away. Dropping a Rust future stops the work itself at its current `.await`. Neither undoes a side effect already
performed remotely: that needs idempotency keys and reconciliation in both languages (Chapter 8.4).

**8. Why `poll` must not block, and what "block" includes.** The executor thread is shared by every task on it; while
one `poll` blocks, none of them run (`ch06-03`: a heartbeat 194 ms late). Blocking includes blocking system calls (file
I/O, `getaddrinfo`), waiting on a contended `std::sync::Mutex`, `thread::sleep`, a synchronous channel `recv`,
`block_on` of another future, and long computations: anything that keeps the thread from returning.

### Debugging exercise (`Refresh`)

1. The check and the publish are two separate steps under two separate locks. Interleaving: the task locks `result`,
   finds `None`, unlocks. The loader stores the snapshot in `result`, then locks `waker`, finds `None` (or a stale
   waker from an earlier poll), and wakes nobody. The task then stores its waker and returns `Pending`. Nobody will
   ever wake it. When the loader happens to finish before the first check, the task sees the result at once: hence
   "sometimes instantly, sometimes never."
2. Publish first, then check again: store (or refresh) the waker, *then* look at `result` once more before returning
   `Pending`. The loader stores the result first and then takes and wakes the waker. With that order, every
   interleaving is covered: either the task's second check sees the result, or the loader sees the task's waker. No
   lock is held while the other is taken. (Putting both fields under one mutex, as `Delay` does, is the simpler fix.)
3. With several tasks sharing one `Refresh` through `Arc`, a single waker slot holds only the last poller's waker: every
   other task hangs. And `take()` hands the snapshot to whichever task polls first; the others find `None` forever. The
   design needs a list of waiters and a result that every waiter can read (clone an `Arc<Snapshot>`), which is the
   `WaitFor` design of `ch02-07`, or `futures::future::Shared`.

### Selected exercises

- **Beginner.** Without `wake_by_ref`, `Countdown` returns `Pending` having arranged nothing. `block_on` parks the thread
  and nothing ever unparks it: the program hangs, using no CPU. That's the lost wakeup in its purest form.
- **Intermediate (`Yield`).** Each loop iteration that awaits `Yield` hands control back to the parent, which polls the
  other branch, so the two loops interleave. Fairness in cooperative scheduling is voluntary: a loop that never yields
  starves its sibling and every other task on the thread. That's Chapter 12.6's `ch06-06` in miniature.
- **Systems (one timer thread).** A `BinaryHeap` of `(deadline, id)` entries plus a map from id to waker, guarded by a
  mutex, with a `Condvar` the timer thread waits on until the earliest deadline. Registering is O(log n) plus a notify
  when the new deadline becomes the earliest. Cancelling is cheapest done lazily: remove the waker from the map (O(1)),
  and let the heap entry expire harmlessly. Chapter 12.5's `ch05-03` is this design; its costs (a cross-thread wake per
  expiry, cancelled entries lingering) are why Tokio uses a timing wheel on its workers.

---

## Chapter 12.3 — async fn Becomes a State Machine

### Interview & architecture questions

**1. Compiling `async fn foo() {}`.** Lowering turns it into `fn foo() -> impl Future<Output = ()>` returning an
`async move` block. The block's type is an anonymous coroutine; after borrow checking, a MIR pass rewrites the body
into a resume function (the `poll`) with one state per suspension point and fields for everything live across each
one. Calling `foo()` at run time only constructs the coroutine value in its `Unresumed` state; no code from the body
runs. Its future is 1 byte because there's nothing to store (no arguments, no locals across an `.await`): just the
state tag.

**2. What decides a future's size.** Roughly: a prefix (captured arguments, the state tag, and saved locals that are
live in more than one state) plus the largest per-state set of saved locals, where fields of different states can
overlap. Two children awaited in sequence are never alive together (MIR's `storage_conflicts` says so), so they share
bytes: 1,028. Joined children are both alive for the whole join, plus the join's output slots: 2,072.

**3. The coroutine layout of `step`.** Saved locals `_s0` (`a`), `_s1` (first awaitee), `_s2` (`b`), `_s3` (second
awaitee). Variants: `Unresumed` (created, never polled; holds only the arguments, here `x`), `Returned` (completed;
polling again panics with "`async fn` resumed after completion", `ch03-15`), `Panicked` (a poll panicked; polling again
panics with "resumed after panicking"), `Suspend0` = `[_s0, _s1]`, `Suspend1` = `[_s0, _s2, _s3]`. The storage
conflict matrix shows `_s1` never overlaps `_s2`, so they may share storage.

**4. 40 bytes vs 24.** rustc lays the coroutine out as a prefix shared by all states (the argument `x`, the tag, and
`a`, which is live in both suspend states) followed by per-state fields. `x` stays in the prefix for the future's whole
life even though only `Unresumed` needs it, and the tag's position adds padding. A hand-written enum is laid out variant
by variant, so its largest variant (`a`, `b`, the awaitee) plus the tag packs into 24 bytes [RUSTC]. Neither layout is
guaranteed; coroutine layout optimization is ongoing compiler work.

**5. `MutexGuard` across `.await`.** `std::sync::MutexGuard` is `!Send` because the lock must be released by the
thread that acquired it [LIB]. Held across an `.await`, the guard is stored in the future, which makes the future
`!Send`. The design bug is worse than the type error: the lock stays held while the task is suspended, for as long as
the awaited operation takes, and any other task that tries to lock it blocks its whole executor thread. On a
single-threaded executor, that can be a deadlock (the debugging exercise).

**6. Recursion needs boxing.** A recursive `async fn` would contain its own future type as a field: an infinitely sized
type (E0733). `Box::pin(depth(n - 1)).await` stores a pointer instead. Cost per level: one heap allocation of the child
future's size (16 bytes in `ch03-09`) and the allocator calls. It moves recursion from the stack to the heap; unbounded
depth still needs a limit.

**7. `async fn` in traits and `dyn`.** Each implementation's `async fn` returns its own anonymous future type, with its
own size. A vtable needs one signature, so the trait isn't dyn-compatible (E0038). Workarounds: return
`Pin<Box<dyn Future<Output = T> + Send + 'a>>` from the trait method (what the `async-trait` crate generates: one
allocation per call); dispatch over an `enum` of known implementations; or stay generic (static dispatch).

**8. The Send bound problem.** With `async fn` in a trait, generic code can't require the returned future to be `Send`:
the type is anonymous, so there's nothing to put a bound on (E0277 in `ch03-12`). Declaring the method as
`fn fetch(&self, id: u64) -> impl Future<Output = String> + Send` makes `Send` part of the trait's contract: every
implementation must return a `Send` future (implementations may still write `async fn`; the compiler checks it), and
callers can rely on it. Return type notation (RFC 3654) would let callers add the bound instead; it's unstable on 1.98.

**9. Cancellation safety.** A function is cancellation-safe if dropping its future at any `.await` leaves the system in
a valid state. `payout_v1` isn't: dropped during the fraud check, it leaves the amount reserved forever (`ch03-14`). The
fix makes the reservation a guard whose `Drop` releases it unless `commit()` was called (`payout_v2`), or restructures
the function so nothing needs undoing if it stops (check before reserving). A crash runs no destructors, so a
reconciliation job remains the backstop.

### Debugging exercise (the `CACHE` deadlock)

1. It compiles because nothing requires the future to be `Send`. It must be running on a single-threaded runtime (a
   current-thread Tokio runtime, a `LocalSet`, or `block_on`), where tasks aren't moved between threads. Spawned on a
   multi-threaded runtime, the `MutexGuard` held across `.await` would be a compile error.
2. Task A calls `config()`, locks `CACHE`, finds `None`, and awaits `fetch_config()`, which returns `Pending` with the
   lock still held. Task B calls `config()` and blocks the executor thread in `CACHE.lock()`, a `std` mutex that blocks
   the OS thread. Task A's HTTP response arrives, but the only thread that could poll task A is blocked waiting for the
   lock that task A holds: deadlock. Tests never have two concurrent callers, so they pass.
3. (a) **No lock across `.await`:** lock, clone the cached value if present, unlock; otherwise fetch without the lock,
   then lock and store (keep the first or the latest value, and say which). Trade-off: concurrent cold callers fetch
   several times (a small thundering herd at startup), but nothing can deadlock and no lock is held across I/O.
   (b) **Single flight:** later callers wait for the first fetch instead of starting their own:
   `tokio::sync::OnceCell::get_or_init`, or a shared in-flight future plus waiters, as in `ch02-07`. Trade-off: one
   fetch, but you must decide what happens when the first fetch fails (the next caller retries) or its caller is
   cancelled (the initialization must not be lost with it).

### Selected exercises

- **Beginner (sizes).** Measured in listing `answers-ch03-sizes.rs` (debug and release identical): a `String` and a
  `u32` passed as **arguments** and held across one `.await`: **64 bytes**, not the ≈32 you'd predict. The arguments
  are stored in the prefix as captured upvars for the future's whole life, *and* the body's bindings are saved again as
  locals across the `.await`: 32 bytes each [RUSTC]. The same function with the `String` created inside the body is 40
  bytes (the `u32` argument, the tag, the `String` local, the awaitee). A `[u64; 4]` held across two consecutive
  `.await`s is 48 bytes: the array is live in both suspend states, so it's in the prefix with the tag, and the two
  1-byte awaitees share a slot. Lesson: large arguments to long-lived `async fn`s may cost twice; take them by reference
  or build them inside.
- **Intermediate (three responses).** Holding 1, 2, and 4 KiB responses until the end makes the last suspension store
  all three: about 7 KiB of future. Reducing each response to the few fields you need before the next `.await` leaves
  only those fields in later states, and the byte arrays die before the suspension: the future shrinks to roughly the
  size of the largest single response plus the extracted fields. (Measure with `size_of_val`; exact bytes depend on the
  layout, as question 4 shows.)
- **Systems (drop glue).** The coroutine's drop glue (`drop_in_place::<{async fn body of ..}>` in MIR) reads the state
  discriminant and drops only the fields that are live in that state: the `String` is dropped if the future is
  suspended at the `.await` that saves it, and not in `Unresumed` (unless it's an argument), `Returned`, or `Panicked`.
  That's what makes cancellation precise.

---

## Chapter 12.4 — Pin and Self-Referential Futures

### Interview & architecture questions

**1. Why a self-borrowing future can't move.** Its state stores a local and a reference to that local. A move in Rust
is a bitwise copy to a new address with nothing fixed up, so the reference would still point at the old location:
freed or reused memory (`ch04-01` under Miri). In Java and Kotlin, the GC updates every reference when it moves an
object, and Kotlin's continuations are heap objects anyway, so self-references cost nothing and need no `Pin`.

**2. The guarantee.** From the moment a value is pinned until its destructor runs: (1) it won't be moved, and (2) its
memory won't be invalidated or repurposed (freed, reused, overwritten) until `drop` runs; if `drop` never runs, the
memory stays valid forever. Unless the type is `Unpin`, in which case the promise means nothing. The promise is made by
whoever creates the `Pin` (`Box::pin`, `pin!`, `Pin::new_unchecked`); it's relied on by the pinned type's own code (the
generated `poll`, an intrusive list node).

**3. `Unpin`.** An auto trait meaning "moving this value after it's been pinned is harmless." `Box<T>` is `Unpin` for
every `T` because moving a box moves the pointer; the `T` stays at its heap address. Every future from `async` is
`!Unpin` because the compiler doesn't analyze whether a particular state machine actually borrows from itself; it marks
them all (`ch04-09`: an `async { 1 + 1 }` with no borrows and no awaits is still `!Unpin`).

**4. `poll(self: Pin<&mut Self>)`.** The first poll may create self-references (a borrow of a local across an
`.await`), and every later poll relies on them. Requiring `Pin` at every call makes the caller prove the future is
pinned, and keeps it pinned. Before the first poll, a generated future is in `Unresumed` and holds only its arguments:
no self-references, so building, returning, and storing unpolled futures is fine.

**5. `Pin::new`, `pin!`, `Box::pin`.** `Pin::new(&mut x)` is safe only for `T: Unpin`, lives wherever `x` is, and pins
nothing in practice: use it for `Unpin` leaves. `pin!(fut)` pins on the current stack frame at no cost; the future
can't outlive the function: use it to poll inside one function (`block_on`, `select` loops). `Box::pin(fut)` costs one
allocation and gives an 8-byte handle that can be stored, returned, or sent to another thread: executors, struct fields,
`dyn Future`, recursion.

**6. Pin projection.** Going from `Pin<&mut Struct>` to a pinned reference to a field (`Pin<&mut Field>`, structural)
or a plain one (`&mut Field`, not structural). For a structurally pinned field: the struct may be `Unpin` only if that
field is; its `Drop` must not move the field; it must uphold the drop guarantee for the field; it must offer no API that
moves the field out while pinned (no `Option::take` on it); and it must not be `#[repr(packed)]`.

**7. One safe line, UB elsewhere.** `impl<F> Unpin for WithBudget<F> {}` tells everyone that a pinned `WithBudget` may
be moved, so safe code can get `&mut` through `Pin::get_mut` and move it. The `unsafe` projection's argument ("`inner`
never moves") is now false, and a self-borrowing inner future dangles (`ch04-06`: a use-after-free reported inside the
inner `async fn`). The fault belongs to the `unsafe` code's author: its soundness depended on the absence of safe code
in its own module, and it didn't defend that. The robust fix makes the dangerous line impossible:
`pin-project-lite` generates the conditional `Unpin` impl, and a manual one then conflicts (E0119, `ch04-13`).

**8. What the drop guarantee enables.** Intrusive waiter lists: each waiting future contains its own list node and
links its own address into the notifier's list on first poll. No allocation per waiter (`ch04-10`: a 40-byte future).
It needs both halves: no move keeps the address in the list correct, and "memory not reused before `drop`" lets `Drop`
unlink the node before the memory can be reused. Without the unlink (`ch04-11`), the list keeps a dangling pointer:
use-after-free under Miri.

**9. `&mut T` for `T: !Unpin`.** rustc omits `noalias` (and `dereferenceable`) on such parameters (`ch04-12`'s IR),
because a pinned `!Unpin` value may legitimately be aliased (by its own self-references, by an intrusive list) while a
`&mut` to it exists. It's a compiler workaround pending an explicit language mechanism (`UnsafePinned`, RFC 3467). The
cost is lost optimization in larger functions.

### Debugging exercise (`Retrying`)

1. `this.inner.take()` moves the inner future out of the pinned struct into the local `fut` (move 1), it's polled at
   that new stack address, and `this.inner = Some(fut)` moves it back (move 2): two moves of a possibly self-referential
   future on every `Pending` poll. The first `poll` pins it at the local's address; the move back invalidates that.
2. The tests used `Unpin` inner futures (`ready()`, hand-written leaves). Moving those is harmless, so there was nothing
   for Miri to detect. To catch it, the inner future must borrow its own local across an `.await` (a self-referential
   `async fn`) and must be polled at least twice: once to create the self-reference, once after the move.
3. With `pin-project-lite`:

   ```rust,ignore
   pin_project_lite::pin_project! {
       struct Retrying<F> { #[pin] inner: Option<F>, attempts: u32 }
   }
   impl<F: Future> Future for Retrying<F> {
       type Output = Option<F::Output>;
       fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
           let mut this = self.project();
           let Some(fut) = this.inner.as_mut().as_pin_mut() else { return Poll::Ready(None) };
           match fut.poll(cx) {
               Poll::Ready(v) => {
                   this.inner.set(None); // dropped in place: allowed on a pinned field
                   Poll::Ready(Some(v))
               }
               Poll::Pending => Poll::Pending,
           }
       }
   }
   ```

   (Unverified sketch; it's the `Fuse` exercise's pattern.) The future is polled where it lives and dropped where it
   lives.

### Selected exercises

- **Intermediate (`Fuse`).** `Pin::set(None)` on a pinned `Option<F>` drops the old `F` in place and writes `None`:
  the pinned value is destroyed, not moved, which the drop guarantee allows. `take()` would move the pinned `F` out to
  a new location while it may hold self-references, which is exactly what pinning forbids.
- **Advanced (`Select2` returning the loser).** A polled `!Unpin` future can't be returned by value, because returning
  it moves it. Either require `B: Unpin` (what `futures::future::select` does), or take and return the futures boxed
  (`Pin<Box<B>>`: moving the box doesn't move the future), or return a pinned reference whose lifetime ties the loser to
  the place it's stored.
- **Systems (allocations per waiter).** Predicted from mechanism: the intrusive `Notify` allocates nothing per waiter
  (the node is inside the future). A `Mutex<Vec<Waker>>` allocates as the vector grows (about log₂ n reallocations for
  a burst of n) and keeps its peak capacity; waker clones for `Arc`-based wakers are refcount increments, not
  allocations. Cancellation: O(1) unlink for the intrusive list; a linear search (or a stale entry left behind) for the
  vector. Measure both with the counting allocator to confirm.

---

## Chapter 12.5 — Wakers and Executors: Build One from Scratch

### Interview & architecture questions

**1. Executor, reactor, timer.** The executor owns tasks and a run queue and polls them, giving each a waker whose
`wake()` re-queues that task. The reactor maps I/O registrations to wakers and is where the thread blocks
(`epoll_wait`) when nothing is runnable. The timer maps deadlines to wakers. The `Waker` (a data pointer and a vtable)
is the only connection: the reactor and timer store wakers and call them without knowing what a task is, and the
executor doesn't know what its tasks are waiting for.

**2. The four vtable functions.** `clone` makes an independent waker for the same task (usually a refcount increment).
`wake` schedules the task **and** releases this waker's resources (it consumes the waker). `wake_by_ref` schedules the
task and releases nothing. `drop` releases the resources of a waker that's discarded without waking. Every count taken
by `clone` must be released by exactly one `wake` or `drop` (`ch05-02` ends at strong count 1, and Miri checks it).

**3. `wake()` on a finished task.** Wakers outlive tasks: a timer fires for a task that was cancelled, a channel sender
wakes a receiver whose task already completed, an I/O event arrives after the connection's task ended. `Waker` is
`Send + Sync` and may be called any number of times from any thread, so the executor must treat a wake of a finished
task as a no-op (`ch05-03`: the future slot is `None`, so the task is skipped).

**4. Clear `queued` before the poll.** A wake can arrive *during* the poll (a child completes on another thread, or the
future wakes itself). With `queued` already cleared, that wake sets it and re-queues the task, so the new event gets a
poll. If the flag is cleared *after* the poll, a wake during the poll sees `queued == true`, does nothing, and then the
executor clears the flag: the task is `Pending`, not in the queue, and never polled again (the debugging exercise).

**5. `block_on` and the missed wake.** `thread::park` has a token: an `unpark` that happens before `park` sets the token,
and the next `park` returns immediately [LIB]. So a wake between "`poll` returned `Pending`" and "call `park`" isn't
lost. Spurious returns from `park` are harmless: the loop simply polls again.

**6. `Rc` in tasks, `Send + Sync` wakers.** `Waker` is `Send + Sync` by definition: any future may hand its waker to
another thread. In `ch05-04` the waker carries only a task **id** and pushes it onto a thread-safe ready queue; the tasks
themselves live in a `Slab` owned by the single runtime thread and never move between threads. So the futures can hold
`Rc` and be `!Send`; only the waker's data must be thread-safe.

**7. Task vs thread cost.** Measured in `ch05-05`: a spawned task is about 2 allocations (the boxed future and the task
structure) and about 160 ns; Tokio makes it one allocation [LIB]. An OS thread spawned and joined was about 36 µs:
three small allocations (the handle, the result packet, the boxed closure) that don't matter, plus a 2 MiB stack
`mmap`, a `clone(2)`, kernel scheduling, and a futex wait to join.

**8. Deterministic simulation.** An executor that uses virtual time (when nothing is runnable, jump the clock to the
next deadline) and seeded scheduling (pick among ready tasks with a seeded random generator). The same seed replays the
same interleaving, so a failing schedule becomes a reproducible test. It finds *logical* races between tasks, such as
check-then-act across an `.await` (`ch05-07`'s double charge in 183 of 1,000 seeds), which involve no data race at all;
a thread sanitizer finds unsynchronized memory accesses, and on one thread there are none.

**9. `join_all` over 1,000 futures, 500 wakes before it runs.** Without wake deduplication: about 500 queue entries and
as many task polls, each re-polling every pending child with a naive `join_all`: `ch05-06` measured 502 polls and
252,000 child polls. With deduplication: 3 task polls (start, after each batch) and 2,500 child polls (1,000 + 1,000 +
500). With per-child wakers (`futures`' `join_all` above 30 children uses `FuturesOrdered`): only woken children are
polled, 2 per child, 2,000 in all (`ch05-08`).

### Debugging exercise (the lost task)

1. The flag is cleared *after* polling. The task was queued by a wake, so `queued == true` while it's being polled. The
   future returns `Pending` having registered its waker with some event source; that event fires on another thread
   before `task.queued.store(false)` runs. The waker's `swap(true)` returns `true` ("already queued"), so it sends
   nothing. Then the executor stores `false`. The task is `Pending`, not in the queue, and its wake has been spent:
   nothing will ever poll it again.
2. Move `task.queued.store(false, Ordering::Release)` to the top of the loop body, before the poll (as `ch05-03` does).
   Then any wake that happens during or after the poll finds `queued == false`, sets it, and re-queues the task.
3. With the fix, a task can be re-queued while it's still being polled. In this single-threaded executor that's
   harmless: the next `recv` happens after the current poll finishes, and one extra poll is allowed (futures tolerate
   spurious polls). In a multi-threaded executor, a second worker could pop the re-queued task while the first is still
   polling it. The `Mutex` around the future keeps that memory-safe, but the second worker **blocks** on the lock, which
   is the thing an executor must never do. Production runtimes use a task state word instead: a wake during `RUNNING`
   sets `NOTIFIED`, and the worker that finishes the poll re-schedules the task itself (Tokio's design [LIB]).

### Selected exercises

- **Intermediate (`yield_now` and fairness).** A `yield_now` future returns `Pending` once after waking itself, which
  sends the task to the back of the run queue. Two CPU-heavy tasks that yield every chunk share the thread; a third
  task's latency is bounded by the chunk length. Without yields, the third task waits for a whole computation (Chapter
  12.6's `ch06-06`: 143 ms without yields, 1 ms with).
- **Advanced (timers in the executor).** When the queue is empty, compute the earliest deadline and `recv_timeout` until
  then; expire due timers after each wait. Cross-thread wakes still work (they send on the channel, which ends the
  `recv_timeout` early). A cancelled sleep's entry stays in the heap until it expires unless you track cancellation, so
  a burst of cancelled timeouts (every request that finished early) leaves garbage; it's harmless but costs memory and
  heap operations.
- **Architecture (simulating merchant-notify).** Abstract time (a `Clock` trait, as the Part XII review's fixed PR
  does), the network (a transport trait with an in-memory implementation that can drop, delay, and reorder), randomness
  (a seeded generator passed in), and scheduling (run the service's tasks on the simulation executor, or on `turmoil`,
  which simulates hosts and networks for Tokio code [LIB]). Building on `turmoil` saves the network model; building
  your own gives full control of scheduling. A failing seed from CI is logged with the scenario; replaying it locally
  gives the same trace, which goes into the bug report and becomes a regression test.

---

## Chapter 12.6 — OS Threads vs Green Threads vs Async Tasks

### Interview & architecture questions

**1. Where suspended state lives.** OS threads and green threads keep it on a stack (a large reserved one, or a small
growable one owned by a runtime); async tasks keep it in the future, sized at compile time. Consequences: stacks must
fit the deepest call chain (so they're large or growable) while futures are exact but fixed; with a stack, any function
can stop anywhere, so blocking code works, while a future can stop only at `.await`, so code between awaits must not
block; and because async functions stop only at `.await`, their callers must be async too: function coloring.

**2. Virtual vs resident.** The thread's 2,064 KiB is mostly an untouched stack reservation: address space, not RAM.
Resident, an idle thread costs 9.6 KiB and a task 0.28 KiB, about 34× apart, not 8,500×. In practice a
thread-per-connection server is limited by kernel resources (one kernel task per thread against `pids.max`,
`vm.max_map_count`, `threads-max`) and address-space policies long before resident memory runs out: at 100,000
connections, about 1 GB resident but 200 GB of reservations and 100,000 kernel tasks.

**3. 8 µs vs 0.12 µs.** A thread round trip includes, per direction, a futex wake of the blocked receiver, the kernel
scheduling it (often on another core, with an inter-processor wakeup), and the sender blocking in its own `recv`
(another futex call and a context switch): at least four system calls and two context switches per round trip, plus
cache effects. A task round trip is a buffer write, a waker call that pushes the other task onto a ready queue, a
`Pending` return, and a poll: function calls in user space, no kernel entry. Measured 7.7–9.6 µs vs 115–118 ns over
three runs.

**4. Go.** The netpoller makes blocking-style socket code event-driven underneath: sockets are non-blocking, a goroutine
that would block is parked and its descriptor registered with epoll/kqueue, and it's made runnable when the descriptor
is ready. Goroutine stacks start at 2 KiB and grow by copying. A system call that really blocks keeps its OS thread
(M); the runtime hands the goroutine's logical processor (P) to another thread so other goroutines keep running. A cgo
call switches to a system stack, because C can't run on a small, movable goroutine stack, which makes cgo calls far more
expensive than Go calls.

**5. Java virtual threads.** A virtual thread runs mounted on a carrier platform thread (a `ForkJoinPool` with one
carrier per core by default). When it blocks in a JDK call that has been retrofitted (sockets, `j.u.c` locks, sleep), it
unmounts: its frames are copied to heap stack chunks, and the carrier runs another virtual thread; when ready, it mounts
again on any carrier. **Pinning** is when it can't unmount and blocks its carrier: inside `synchronized` (until JEP 491,
JDK 24, removed that case) or with a native frame on its stack (still). Operations that can't unmount, such as many file
system calls, temporarily add carrier threads to compensate.

**6. Why M:N worked for Go and JDK 21 but not for early Rust or C.** M:N needs the runtime to control every blocking
operation and to manage stacks. Go owns both: its own system-call wrappers and netpoller, and a compiler that records
every pointer so stacks can be moved and grown. JDK 21 owns both through the JVM: stack chunks are heap objects the GC
understands, and the JDK's blocking APIs were rewritten to unmount. A library in C, or in Rust, can't intercept a
blocking call made by arbitrary code (a C library's `read`, a `std` mutex), and can't move native stacks containing raw
pointers. That's also RFC 230's argument for removing Rust's green threads, alongside embeddability.

**7. Function coloring.** Async functions can be awaited only from async code, so the "async" property spreads to
callers. Ways across in Rust: make the caller async (wrong when a public synchronous API can't change); `block_on` from
synchronous code (wrong on a thread that's running async tasks: Tokio panics with "Cannot start a runtime from within a
runtime", `ch06-05`, and a single-threaded executor deadlocks); `spawn_blocking(f).await` for blocking work from async
code (wrong when the work must be cancellable: it can't be once started); Tokio's `block_in_place` (wrong on a
current-thread runtime, where it panics, and as a general habit).

**8. Cancellation compared.** Rust async: drop the future (or `abort` its task); it stops at its current `.await`,
destructors run, and the cancelled code needs no cooperation. Java: `interrupt()` makes interruptible blocking calls
throw `InterruptedException`; code that doesn't block or check the flag keeps running; `Thread.stop` is gone (it throws
since JDK 20). Go: cancel a `context.Context`; the goroutine must check `ctx.Done()`. Erlang: an exit signal kills the
process immediately; that's safe because processes share no memory, and links and supervisors handle the aftermath.

**9. A CPU-bound loop in a Tokio task.** It runs to completion inside one `poll`, holding its worker thread: every task
queued on that worker waits (`ch06-06`: a heartbeat 143 ms late). Tokio's cooperative budget doesn't help because it's
consumed only by operations on Tokio's own resources (socket reads, channel receives), and a pure computation performs
none [LIB]. Fixes: `spawn_blocking` (or a dedicated pool) for long computations, a rayon pool for data parallelism, or
chunking with `yield_now().await` between chunks.

### Debugging exercise (`nightly_digest`)

1. The loop has no `.await`, so the whole digest runs inside a single poll: 2 million × 0.5 µs ≈ 1 s on one worker.
   Other workers keep running their own tasks, which is why CPU looks idle and only some deliveries suffer: those whose
   tasks were queued on (or woken onto) the busy worker and weren't picked up by another worker in time. Those wait up to
   about a second [LIB: how many depends on the runtime's work-stealing policy].
2. The cooperative budget counts operations on Tokio resources; the loop performs none, so the task is never asked to
   yield, and even a forced yield request would have nowhere to take effect without an `.await`.
3. Three fixes:
   - `spawn_blocking(move || compute(snapshot))` with an **owned** snapshot (an `Arc` of an immutable snapshot, or a
     copy): the workers stay free; the digest is consistent (one point in time); memory is the snapshot's size for the
     job's duration.
   - **Chunking** with `yield_now().await` every few thousand sessions (a few milliseconds of CPU): stays on the async
     side and bounds added latency to one chunk. Iterating a snapshot across yields keeps it consistent but holds its
     memory across `.await`s (and it must be `Send`); iterating the live store with a cursor keeps memory bounded but
     the digest is no longer a point-in-time view.
   - A **rayon** pool: parallel over the snapshot, finishes fastest, and uses cores the runtime also wants; send the
     result back through a oneshot channel.

   Bounded memory: the cursor-based chunking. Consistency: any design built on a snapshot. Choose by what the digest is
   for; for a nightly report, a snapshot on `spawn_blocking` is the usual answer.

### Selected exercises

- **Beginner (timeout around `spawn_blocking`).** The caller gets `Elapsed` after 50 ms, but the hashing continues to
  completion on the blocking pool: a `spawn_blocking` closure can't be cancelled once it starts, and its result is
  simply dropped. A job holding a database connection keeps holding it until it finishes, so timeouts around blocking
  work don't free resources. Bound the pool (a semaphore) and design for work that outlives its caller.
- **Intermediate (yield interval).** In the verified run, 1,025 yields in about 141 ms means roughly 51 million
  iterations at about 2.8 ns each, so a yield every 50,000 iterations is about 140 µs of CPU between yields, and it gave
  a worst delay of 1 ms at no measurable cost. A yield costs on the order of 100 ns (the task round trip), so yielding
  every ~100 µs–1 ms of CPU costs well under 1% and caps the added latency at that interval. That's the rule of thumb:
  choose the interval from the latency you can tolerate, then check the overhead.
- **Architecture (merchant-notify ADR).** Context: 600,000 mostly idle connections, 100,000 per pod. Options: Tokio
  multi-threaded (work stealing balances hot merchants; tasks must be `Send`; the ecosystem fits), thread-per-core
  (no `Send`, no cross-core wakes, locality; hot merchants and slow clients can overload one core; cross-core fan-out
  needs messaging), Java virtual threads (the Java team's existing code and libraries; KiB per idle connection that
  grows with depth; pinning risks in native TLS paths). Decision (one defensible answer): Tokio multi-threaded, with
  size budgets per connection task and admission control. What would reverse it: a measurement showing cross-core wake
  and stealing overhead dominating CPU at 100,000 connections (favoring thread-per-core), or the Java team measuring
  virtual threads within budget at 100,000 connections per pod on their existing code.

---

## Part XII Review — Capstone: the merchant-notify PR

**Defects** (listing `review-01-notify-pr.rs`; the fixed version `review-02-notify-fixed.rs` has a test for the ones
marked ✔):

| # | Defect | Rule broken | Impact |
|---|---|---|---|
| 1 | `AckWait::poll` stores a waker only once (`if st.waker.is_none()`) | Wake the waker from the **latest** poll (12.2) | ✔ **Hang**: when the future is polled by another task later, the ACK wakes the wrong one; deliveries wait for the timeout (the March 2026 incident) |
| 2 | Interest in the ACK is registered **after** the send completes (`wait` after `send`) | Publish interest before the event can happen (12.2) | **Hang/duplicate**: a fast terminal's ACK finds no entry and is dropped; the delivery times out and is retried |
| 3 | `Timeout` checks `Instant::now()` only when polled; nothing wakes the task at the deadline | The `Pending` obligation covers the timeout itself (12.2) | **Hang**: a lost ACK means no poll, so the "5 s timeout" never fires |
| 4 | `impl<F> Unpin for Timeout<F> {}` next to an `unsafe` `map_unchecked_mut` projection | Structural pinning; `Unpin` is safe (12.4) | **UB waiting for a refactor**: any code that moves a polled `Timeout` corrupts a self-referential inner future (`ch04-06`) |
| 5 | `delivered` pushed **before** sending, and kept even when the send fails or no ACK arrives | Record effects after they happen; at-least-once semantics | ✔ **Corrupt record**: events marked delivered that no terminal received |
| 6 | `self.delivered.lock()` guard held across every `.await` | No lock across `.await` (12.3) | **Stalls/deadlock**: deliveries serialize; on one thread a second delivery blocks the executor; the future is `!Send` |
| 7 | `std::thread::sleep` for backoff inside an `async fn` | `poll` must not block (12.2, 12.6) | ✔ **Stalls other tasks** for 100, 200, 400 ms per failing merchant |
| 8 | `[u8; 65536]` frame buffer held across `.await`s | Future size is a budget (12.3) | ✔ **Memory**: ≈64 KiB per in-flight delivery (the fixed future is 136 bytes); stack overflow when pinned on a small stack |
| 9 | `ack()` never removes entries; timed-out and cancelled waits never remove theirs | Cancellation safety; cleanup on every exit (12.3) | ✔ **Memory leak**: one map entry per event, forever |
| 10 | `ack()` wakes while holding both locks | Wake after releasing locks (12.2 §9) | Contention; a waker that runs executor code under your lock |
| 11 | `wait()` uses `insert`, silently replacing an existing waiter for the same `event_id` | Invariants must be checked | **Hang**: the first waiter is never woken |
| 12 | Dropping `deliver` mid-flight leaves a "delivered" record and a pending entry | Cancellation safety (12.3) | ✔ **Corrupt record + leak** when a connection closes |
| 13 | `deliver_blocking` calls `futures::executor::block_on` from async handlers | Coloring: never `block_on` on an executor thread (12.6) | **Deadlock** on a single-threaded runtime if the delivery needs that executor; blocks a worker otherwise |
| 14 | `Connection::send` returns `impl Future` without `+ Send` | The Send bound problem (12.3) | ✔ Generic callers can't spawn `deliver` on a multi-threaded runtime |
| 15 | `encode` slices a fixed buffer: a payload over ≈64 KiB panics; `format!` allocates a header per event | Validate input; no panics on data (Part VIII); allocation per message (9.2) | **Crash** of the connection task on a large event; an allocation per delivery |
| 16 | Time read directly with `Instant::now()` and `thread::sleep` | Inject the clock (12.5 §9) | ✔ Untestable: no test can exercise the timeout or backoff without waiting real seconds |
| 17 | Returns `bool`: "send failed" and "no ACK" look the same | Error taxonomy (8.4) | Callers can't decide between reconnecting and retrying |

**Classification.** *Hangs*: 1, 2, 3, 11 (and 13 as a deadlock). *Corrupt record of delivery*: 5, 12. *Stalls other
tasks*: 6, 7, 13. *Memory*: 8, 9, 12. *UB waiting for a refactor*: 4. Everything else is testability, API, or input
validation. The ranking by consequence: 5 and 12 first (merchants told an event was delivered when it wasn't, with no
retry), then the hangs (1, 2, 3), then the executor stalls (6, 7), then the latent UB (4), then memory (8, 9).

**The ordering bug (question 3).** A terminal on a fast network can ACK before the sender's `send` future completes
(the demo's `AckingConn` does exactly that). In the PR, `ack()` runs when there's no entry for the event, so the ACK is
ignored; `wait()` then creates a fresh entry that no ACK will ever complete, and the delivery times out (or, with defect
3, hangs). The correct order is **register, then send, then wait**: `register(event_id)` inserts the entry before the
frame can reach the terminal, so any ACK, however early, finds it. It's correct because the registration
*happens-before* the send, and the send happens-before any ACK for it: the same "publish interest, then check" rule as
a waker, at the protocol level.

**The two compile errors (question 4).** `tokio::spawn` requires `Send`. (1) The `MutexGuard` from
`self.delivered.lock()` is alive across the `.await`s, so the future stores a `!Send` guard. (2) `Connection::send`
promises only `impl Future`, not `impl Future + Send`, so generic code can't prove the future it awaits is `Send`. The
PR's test drives `deliver` with `block_on` on one thread, where nothing asks for `Send`, so both stay invisible until
someone spawns it.

**The timeout (question 5).** A future that returns `Pending` must arrange to be woken when polling it again can make
progress. `Timeout`'s progress includes the deadline passing, but it never registers anything for the deadline: it
only compares `Instant::now()` when something else polls it. In the test, the inner future is ready or woken quickly,
so the timeout path is never needed. In production, when the ACK is lost, nothing ever polls the task again, and the
five seconds never elapse from the future's point of view. A correct timeout races the inner future against a timer
future that registers the waker with a timer (`select` with `Clock::sleep`, or `tokio::time::timeout`).

**The new signatures (question 6)**, as in the redesign:

```rust,ignore
pub trait Connection {
    fn send(&mut self, frame: &[u8]) -> impl Future<Output = std::io::Result<()>> + Send;
}
pub trait Clock: Sync {
    fn sleep(&self, d: Duration) -> impl Future<Output = ()> + Send;
}
pub enum Outcome { Acked, NoAck, SendFailed }

impl<K: Clock> Notifier<K> {
    pub async fn deliver<C: Connection>(&self, conn: &mut C, frame: &mut Vec<u8>, d: &Delivery) -> Outcome;
}
```

(Excerpt of `review-02-notify-fixed.rs`.) The per-connection `frame` buffer is reused across deliveries; the `Clock`
makes time injectable (a fake clock in tests, Tokio's timer in production); `register` happens before the send;
`AckWait` refreshes its waker, and its `Drop` removes the entry; `ack()` removes the entry and wakes after releasing
locks; `delivered` is recorded after the ACK, with the guard confined to one statement; the backoff sleeps are
`clock.sleep(..).await`; the timeout is a `select` against `clock.sleep`; the hand-written `Timeout` and its `Unpin` impl
are gone; `deliver_blocking` is gone (admin handlers are async and `.await` the delivery). The five tests cover defects
1, 5, 7 (exact backoff sleeps), 9 and 12 (nothing left behind after no-ACK and after cancellation), and 8 and 14 (size
budget and `Send`).

---

## Part XII Review — Interview mode

1. **`async fn` desugaring and size:** Chapter 12.3, questions 1–2. Calling it constructs the coroutine; `.await`
   expands to a poll loop that yields `Pending` and resumes with a fresh `Context`; the size is the prefix plus the
   largest state (and arguments may be stored twice: `answers-ch03-sizes.rs`).
2. **The `Future` contract:** Chapter 12.2, questions 2–3 and the §4 table. The silent one is `Pending` without a
   registered (current) waker.
3. **`Pin`:** Chapter 12.4, questions 2–4. No run-time cost: `Pin` only withholds `&mut T` from safe code.
4. **Coroutine MIR layout:** Chapter 12.3, questions 3–4. Sequential awaitees don't conflict in `storage_conflicts`, so
   they share bytes.
5. **`Waker` and wake deduplication:** Chapter 12.5, questions 2 and 4, and §10's measurement (252,000 vs 2,500 child
   polls).
6. **How an executor sleeps and is woken:** `park`/`unpark` (futex) or `epoll_wait` with an eventfd registered so
   another thread can interrupt it (Chapter 12.5 §4).
7. **Task vs thread costs:** Chapter 12.6, questions 2–3, and Chapter 12.5, question 7: ≈250 B vs ≈10 KiB resident (2 MiB
   virtual); ≈160 ns vs ≈36 µs to spawn; ≈0.12 µs vs 7.7–9.6 µs per round trip.
8. **8 GB RSS at 100,000 connections:** measure the connection task's future (`size_of_val`) first. The usual cause is
   large buffers held across `.await` inside the per-connection future: 80 KiB × 100,000 = 8 GB, merchant-notify's
   case (Chapter 12.3 §9). Then look at per-connection heap buffers that grew and never shrank (capacity policy), and at
   retained high-water marks in queues.
9. **A password hash on the executor thread:** every task on that worker is delayed by the hash's duration, and at
   enough concurrent hashes every worker is busy (Chapter 12.6 §10: eight workers, 150 ms each, about 53 handshakes per
   second). Find it with per-poll durations (`tokio-console`'s busy time, or a load test that fails on long polls);
   move it to `spawn_blocking` behind a semaphore.
10. **When async is wrong:** CPU-bound services (the fraud library: async adds a poll and forces a runtime into the
    caller) and services with a few hundred connections or concurrency capped by a database pool (the ledger): threads
    are simpler, preemptive, and just as fast (Chapter 12.6 §9).
11. **Cancellation-safe payout:** Chapter 12.3, question 9 and the design exercise. A reservation guard with `Drop`,
    spawning the critical section as its own task if it must not be abandoned, idempotency keys for the external call,
    and reconciliation for crashes.
12. **Rust async vs virtual threads vs goroutines:** Chapter 12.6 §7–§8 matrix. For a million mostly idle WebSocket
    connections: Rust async on Tokio if the team owns the stack and wants the smallest per-connection footprint and no
    GC; Go or Java virtual threads are defensible when the team's code and libraries live there and a few KiB per
    connection fits the budget. The answer should cite memory per idle unit and who owns the blocking code.
13. **ACK-based at-least-once:** register interest before sending; record "delivered" only after the ACK; on timeout,
    resend with the **same** `event_id`; the receiver de-duplicates by `event_id` (idempotent apply), because an ACK can
    be lost after the event was applied. That's the Part XII review's fixed design plus Chapter 8.4's idempotency.
14. **Deterministic simulation:** Chapter 12.5, question 8 and the Architecture exercise. Abstract time, network,
    randomness, and scheduling.
15. **20,000 reconnects in ten seconds:** accept connections cheaply and admit handshakes at a bounded rate (reject the
    rest with retry-after plus jitter); keep expensive verification off the workers (`spawn_blocking` behind a
    semaphore) and make reconnects cheap (resumption tokens); keep heartbeats flowing to existing connections, because
    missing heartbeats turn a blip into a storm; cap per-IP and per-merchant connections. Chapter 12.1 §10 and Chapter
    12.6 §10.
