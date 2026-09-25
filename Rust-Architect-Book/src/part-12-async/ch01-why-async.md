# Chapter 12.1 — Why Async Exists: From C10K to C10M

> **Where this sits:** Part XII · Async Rust · chapter 1 of 6
> **Prerequisites:** Part XI (threads, `Send`/`Sync`, channels). Chapter 1.2's comparison of runtimes.
> **After this chapter you can:** explain why one thread per connection stops scaling, and where exactly it stops;
> describe blocking, nonblocking, readiness-based, and completion-based I/O at the syscall level; read an epoll event
> loop; and say why Rust made async a library feature with no built-in runtime.

---

## Pass 1 · User level — *Why not just use a thread per connection?*

### 1. Problem

A server's job is to wait. A merchant dashboard opens a connection to Meridian's **merchant-notify** service and then,
for minutes at a time, nothing happens. When a payment is captured, an event goes down the connection in a few hundred
microseconds, and then the connection waits again. At peak, merchant-notify holds about 600,000 of these connections,
and more than 95% of them are idle at any moment.

The obvious design gives each connection a thread that calls `read` and blocks until the client says something. The
code is straight-line and easy to reason about, and for a few hundred connections it's the right design. The question
this chapter answers is what it costs per connection, and what breaks first when the count grows. In 1999 Dan Kegel
posed it as **the C10K problem**: how do you serve ten thousand concurrent clients on one machine? The answers he
catalogued are the ancestors of every event loop in use today, from nginx and Netty to Node.js and Tokio.

### 2. Mental model

Two ways to wait for a socket:

```text
 BLOCKING (thread per connection)                  READINESS (one thread, many connections)

 thread 1 ── read(fd 7) ── parked in the kernel     thread ── epoll_wait([7, 8, 9, ...]) ── parked
 thread 2 ── read(fd 8) ── parked in the kernel               │ returns: "8 is readable"
 thread 3 ── read(fd 9) ── parked in the kernel               ├── read(fd 8) → data, handle it
   ...one stack, one kernel task per connection               └── back to epoll_wait
                                                     "where was connection 8 in its protocol?"
                                                      must be stored somewhere else: a STATE MACHINE
```

With blocking I/O, the **thread's stack remembers where each connection is**: which function it's in, which local
variables it has, what it's waiting for. That's why the code is easy to write. With readiness I/O, one thread serves
everyone, so that memory has to live somewhere else: an explicit per-connection state object that the event loop
updates. Writing those state machines by hand is what made event-driven servers hard.

**`async`/`await` is the compiler writing that state machine for you.** You write straight-line code; at each `.await`
the compiler saves exactly the variables that are still needed, and the event loop resumes it later (Chapter 12.3 shows
the generated code). The rest of this Part is the machinery that makes that true.

### 3. Rust code

**Thread per connection, until it breaks.** Listing `ch01-01-thread-per-conn.rs` runs an echo server that spawns one
handler thread per accepted connection, then opens idle connections (connect, send nothing) until something fails. It
reads the process's own memory from `/proc/self/status` along the way. The accept loop, trimmed:

```rust,ignore
for (i, stream) in listener.incoming().enumerate() {
    let mut stream = stream.unwrap();
    let spawned = thread::Builder::new().spawn(move || {
        let mut buf = [0u8; 512];
        // Parked in read(2) until the client sends something or hangs up.
        while let Ok(n) = stream.read(&mut buf) {
            if n == 0 || stream.write_all(&buf[..n]).is_err() {
                break;
            }
        }
    });
    // ... report success, or the spawn error, to main
}
```

Verified output (release build, the Playground's Linux x86-64 sandbox):

```text
baseline:         threads   2 | VmSize    70892 kB | VmRSS 2216 kB
   1 connections: threads   3 | VmSize    72944 kB | VmRSS 2232 kB
 100 connections: threads 102 | VmSize  2243372 kB | VmRSS 3324 kB
 250 connections: threads 252 | VmSize  2552972 kB | VmRSS 4736 kB
 500 connections: threads 502 | VmSize  3068972 kB | VmRSS 7128 kB
thread spawn failed at connection 503: Resource temporarily unavailable (os error 11)
connection 1 still echoes: ping
```

Connection 503 fails. `EAGAIN` from `clone(2)` means the kernel refused to create another task, here because the
sandbox caps the number of tasks per container [OS]. A production host has higher limits, but it *has* limits, and
§6 and §10 show where they usually are. Two other numbers in that table are worth a second look: 3 GB of virtual memory
for 500 idle connections, and 7 MB resident. Section 5 explains both.

**Readiness on one thread.** Listing `ch01-04-mio-c10k.rs` does the same job with `mio`, a thin Rust wrapper over
epoll (Linux), kqueue (BSDs, macOS), and IOCP-based emulation (Windows) [LIB]. The whole server is one loop:

```rust,ignore
while echoed < N {
    poll.poll(&mut events, None)?; // epoll_wait: park until something is ready
    wakeups += 1;
    for ev in events.iter() {
        if ev.token() == LISTENER {
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let slot = conns.vacant_entry();
                        poll.registry().register(&mut stream, Token(slot.key()), Interest::READABLE)?;
                        slot.insert(stream);
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                    Err(e) => return Err(e),
                }
            }
        } else {
            let stream: &mut mio::net::TcpStream = &mut conns[ev.token().0];
            // ... read until WouldBlock, echo each chunk
        }
    }
}
```

The listing raises the open-file limit (each in-process connection costs two descriptors, and the soft limit is 1024),
opens 10,000 client connections from the same process, sends one request on each, and reads every echo:

```text
10000 connections open; threads in process: 2
kernel TCP sockets: inuse 20001 orphan 0 tw 0 alloc 22571 mem 106
10000 echoes from 10000 connections: 1 server thread, 8807 epoll_wait wakeups
VmRSS 2192 kB -> 2456 kB
```

Ten thousand connections, one server thread (the other thread is `main`, acting as all the clients), and a resident-set
growth of 264 KB for the whole process, both ends of every connection included. The limit that stopped the threaded
server at 503 never comes into play.

The price is visible in the code. The handler is no longer a function that reads, then writes, then loops. It's a
reaction to an event, and any state that must survive between events (a half-read frame, a pending response) would have
to be stored in the slab next to the stream. This echo protocol has no such state. Real protocols do, and Chapter 12.3
shows how `async fn` stores it for you.

---

## Pass 2 · Systems level — *What the kernel does in each model*

### 4. Under the hood

**Blocking I/O.** `read(fd)` on an empty socket puts the calling thread on the socket's wait queue and deschedules it
[OS]. When a packet arrives, the kernel's network stack copies it into the socket's receive buffer and wakes the
waiting task, which eventually runs, returns from `read`, and continues. The thread is the unit of waiting. One
connection, one parked thread.

**Nonblocking I/O.** Set `O_NONBLOCK` on the descriptor (`set_nonblocking(true)`), and `read` on an empty socket
returns at once with `EAGAIN`, which Rust surfaces as `ErrorKind::WouldBlock`. That frees the thread, but now it has to
know *when* to try again. Listing `ch01-02-nonblocking-spin.rs` does the naive thing, trying again immediately, while
the data is 20 ms away:

```text
empty nonblocking socket: WouldBlock (Resource temporarily unavailable (os error 11))
19617 read() calls in 20.233339ms to receive 4 bytes
```

Nearly twenty thousand system calls, and a full core burned, to receive four bytes. Busy polling has legitimate uses
(kernel-bypass networking and some low-latency trading systems pin a core to spin on purpose), but as a general strategy
it's the wrong answer. The right one is to ask the kernel to **tell you when a descriptor is ready**.

**Readiness notification: select, poll, epoll.** The three Unix generations differ in who remembers the interest set:

| Mechanism | Interest set lives in | Cost per wait | Limits |
|---|---|---|---|
| `select(2)` (1983, 4.2BSD) | a bitmap you pass in every call | O(highest fd), copied both ways | `FD_SETSIZE`, usually 1024 descriptors |
| `poll(2)` | an array you pass in every call | O(number of fds), copied in | none built in, but linear |
| `epoll(7)` (Linux 2.6, 2002–2003) | the kernel (`epoll_ctl` adds/removes) | O(ready fds) returned | none built in |
| `kqueue(2)` (FreeBSD 4.1, 2000; macOS) | the kernel | O(ready events) | generalizes to timers, signals, processes |

(Dates are from the man pages and kernel release histories.) The important column is the third. With 10,000
connections of which 20 are active, `select` and `poll` hand the kernel all 10,000 on every call; `epoll_wait` returns
the 20.

Listing `ch01-03-epoll-raw.rs` drives epoll directly through `libc`, with Unix socketpairs standing in for TCP
connections, and checks two properties, first natively and then under **Miri** (which models epoll and socketpairs,
though not real TCP):

```text
ready before any data: []
ready after writes to 0 and 2: [0, 2]
level-triggered: ready [7]; read 4 of 8 bytes; ready again [7]
edge-triggered: ready [7]; read 4 of 8 bytes; ready again []
```

The first two lines are the whole idea: register many descriptors once, each with a **token** (here 0, 1, 2), and get
back only the tokens that are ready. The last two lines are the difference between the two epoll modes, and it matters
for correctness:

- **Level-triggered** (the default): "ready" is a *state*. While unread data remains, every `epoll_wait` reports the
  descriptor again. You may read part of the data and come back later.
- **Edge-triggered** (`EPOLLET`): "ready" is an *event*, reported once per transition from not-ready to ready. Read 4
  of 8 bytes and the remaining 4 will **not** be reported again until *new* data arrives. You must drain the socket
  until `WouldBlock`, or that connection hangs.

mio registers descriptors edge-triggered [LIB], which is why the event loop above reads in a `loop` until
`WouldBlock`. Tokio inherits the same rule. Chapter 12.5's reactor follows it too, in a form you've already seen:
*try the operation; only if it says `WouldBlock`, register interest and wait*.

**Completion-based I/O: IOCP and io_uring.** Windows' I/O completion ports (NT 3.5, 1994) and Linux's `io_uring`
(Linux 5.1, 2019) use a different contract. Instead of "tell me when I *can* read," you say "read into this buffer and
tell me when it's *done*." The kernel owns the buffer while the operation is in flight [OS]. That's more efficient for
some workloads (fewer syscalls, batching, true async file I/O). It also collides with a Rust rule you'll meet in
Chapter 12.2: dropping a future cancels it. If a future lends a `&mut [u8]` to the kernel and is then dropped, the kernel
may still write into memory that has been freed or reused. Completion-based Rust runtimes (`tokio-uring`, `glommio`,
`monoio`) therefore take **owned** buffers (`read(buf: Vec<u8>) -> (io::Result<usize>, Vec<u8>)`), giving the buffer
back when the operation completes [LIB]. The readiness model has no such problem: nothing is lent to the kernel between
calls.

### 5. Memory

**What a thread costs.** Listing `ch01-01` measured it. From 100 to 500 connections, each additional handler thread
added **2,064 KiB of virtual address space and about 9.5 KiB of resident memory** ((3,068,972 − 2,243,372) / 400, and
(7,128 − 3,324) / 400). Chapter 12.6's listing `ch06-01-memory-per-unit.rs` repeats the measurement with idle threads
and gets the same 2,064 KiB and 9.6 KiB.

```text
 one Rust-spawned thread (glibc, x86-64 Linux):
 ┌──────────── 2 MiB stack, reserved by mmap [LIB: std's default for spawned threads] ───────────┐
 │ guard page │   untouched: virtual only, no RAM      │  touched pages: a few KiB actually resident │
 └────────────┴────────────────────────────────────────┴─────────────────────────────────────────────┘
 + kernel task struct and kernel stack (kernel memory, not in VmRSS) [OS]
 + thread-local storage, the thread's handle, a slot in the scheduler's run queues
```

Virtual memory isn't free either. It's cheap until something enforces a limit: `ulimit -v`, container memory policies
that count commitments, or `vm.overcommit_memory=2`. And there's a larger surprise in the table: the first 100 threads
added **2.1 GB** of address space, not 200 MB. The extra ≈1.9 GB is 30 × 64 MiB. That matches glibc's per-thread
**malloc arenas**, each of which reserves 64 MiB of address space on 64-bit, with up to 8 × cores of them (32 on this
4-core machine) [LIB]. Nothing is wrong with that; it's what glibc does to reduce lock contention in `malloc`. But it
means "how much memory does a thread use" has at least three answers: resident (~10 KiB here), stack reservation
(2 MiB), and whatever your allocator does per thread.

**What an idle connection costs in the event loop.** In `ch01-04`, the user-space cost of an idle connection was a
`TcpStream` (a file descriptor, 4 bytes) in a slab slot. The resident-set growth for 10,000 connections was 264 KB for
both ends: about 26 bytes per connection. The rest is **kernel memory**: socket structures and buffers, which don't
show up in `VmRSS`. The kernel's `/proc/net/sockstat` line reported 20,001 TCP sockets in use and `mem 106`, which is 106
pages of buffer memory (424 KiB) with almost no data in flight [OS]. Under load that number grows with the data queued
in socket buffers (up to the `tcp_rmem`/`tcp_wmem` limits per socket), which is why C10M-scale servers tune socket
buffers deliberately.

Once connections carry protocol state (a partially parsed request, a pending response), that state costs memory too,
in either model. The difference is where it lives: in a 2 MiB stack reservation per thread, or in a state object sized to
exactly what's needed. Chapter 12.3 measures those state objects for `async fn`: 16 bytes for a function holding one
`u64` argument, 1,026 bytes for one that holds a 1 KiB buffer across an `.await`.

### 6. CPU / OS

**Context switches.** Every blocking thread that becomes runnable costs a kernel context switch: save registers, run the
scheduler, restore another task's registers, and possibly switch address-space state. The direct cost is on the order of
a microsecond or a few, with indirect costs (cold caches and TLB entries, branch predictor state) that can be larger
[CPU][OS] (order of magnitude; Chapter 12.6 measures a thread-to-thread message round trip at about 8 µs on the
Playground, versus about 0.1 µs between two async tasks on one thread). An event loop handling 20 ready connections
per `epoll_wait` pays one syscall and zero context switches for all of them. In `ch01-04`, 10,000 echoes took 8,807
wakeups, so most `epoll_wait` calls returned a single event: under light load, event loops don't batch much. Under heavy
load they batch a lot, and the per-request overhead falls as load rises, the opposite of thread-per-connection.

**The limits a thread-per-connection server hits,** roughly in the order they usually bite on Linux [OS]:

| Limit | Default (typical) | What you see |
|---|---|---|
| Container/cgroup `pids.max` | set by the platform (the Playground's sandbox stops at about 500) | `EAGAIN` from `clone`: `Resource temporarily unavailable` |
| `vm.max_map_count` | 65,530 mappings | Each thread stack is at least 2 mappings (stack + guard), so roughly 32,000 threads, then `clone` fails |
| `kernel.threads-max`, `kernel.pid_max` | depends on RAM / 32,768 or 4,194,304 | `EAGAIN` |
| `RLIMIT_NPROC` (`ulimit -u`) | per user | `EAGAIN` |
| Open files (`ulimit -n`) | soft 1,024 (seen on the Playground) | `EMFILE: Too many open files`: hits event loops too |

The last row applies to both designs: every connection is a file descriptor. Raising `RLIMIT_NOFILE` (as `ch01-04`
does) is standard for any server that holds many connections.

**The JVM's version of `EAGAIN`** is `java.lang.OutOfMemoryError: unable to create native thread`, and it's misleading:
the Java heap is usually fine. The failure is in `pthread_create`, for one of the reasons in the table. §10 is an
incident with exactly that message.

---

## Pass 3 · Architect level — *When is async worth it, and why is it a library?*

### 7. Trade-offs

**Thread-per-connection is not wrong.** For hundreds of connections, or thousands of busy ones, threads are simpler to
write, simpler to debug (a stack trace shows the whole story), and preemptively scheduled, so one slow handler can't
starve the others. PostgreSQL still uses a process per connection. Many internal Rust services are best written with a
thread pool and blocking I/O. The pressure toward async comes from a specific shape of workload:

| Workload shape | Threads | Event loop / async |
|---|---|---|
| Hundreds of connections, CPU-heavy requests | **Good**: simple, preemptive | No gain; CPU work blocks the loop anyway |
| Thousands of busy connections | Fine with a pool; context switches add up | Good |
| Tens of thousands+ of mostly **idle** connections (push, chat, IoT, long polling, WebSockets) | Memory and kernel limits | **Good**: idle cost is a small state object |
| Fan-out to many slow backends per request (API gateway) | One blocked thread per outstanding call | **Good**: waiting costs nothing |
| Embedded, no OS, no threads | Impossible | **Good**: async executors exist for bare metal (e.g. Embassy) [LIB] |

**C10M.** In 2013 Robert Graham argued (the "C10M" talk, Shmoocon 2013) that the next bottleneck at ten million
connections is the kernel itself: per-packet interrupts, the socket layer, the scheduler. The answers at that scale are
kernel bypass (DPDK), batching and completion interfaces (io_uring), and sharding a machine into per-core event loops
with no shared state (thread-per-core runtimes such as `glommio` [LIB]). The progression is the point: first the thread
is too expensive per connection, then the kernel's per-operation overhead is.

**Why async/await in Rust is a library feature with no runtime.** Rust *used to* have a runtime. Before 1.0, `std` shipped
green threads (`libgreen`), M:N-scheduled onto OS threads, with segmented stacks. RFC 230 (2014) removed the green-thread
runtime from `std`, for the reason Chapter 1.2 gave: **embeddability**. A mandatory runtime can't be dropped into a C
program, an OS kernel, a browser engine, or a microcontroller. What replaced it took five years:

| Year | Step |
|---|---|
| 2014 | RFC 230: remove green threads; `std::thread` = OS threads, nothing else |
| 2016 | `futures` 0.1 and Tokio: poll-based futures as a library ("Zero-cost futures in Rust", Aaron Turon, 2016) |
| 2019 | `Pin` stable (1.33); `Future` and `std::task` stable (1.36); `async`/`await` stable (1.39) |
| 2023–2025 | `async fn` in traits (1.75), async closures and `Waker::noop` (1.85) [VERSION] |

The division of labor that came out of it:

- **The language** provides the `Future` trait (in `core`, so it works without an OS), `async`/`await` syntax, and the
  compiler transformation into state machines. Nothing else.
- **Libraries** provide executors, reactors, timers, and I/O types: Tokio, smol, Embassy for embedded, glommio and
  monoio for thread-per-core io_uring [LIB]. (async-std, once Tokio's main rival, was discontinued by its maintainers in
  2025, who pointed users to smol [VERSION].)

The benefit: no runtime cost unless you opt in, and the same `Future` trait from a microcontroller to a 64-core server.
The cost: you choose a runtime, libraries sometimes depend on one, and a mismatch shows up as a panic like "there is no
reactor running" (Part XIII).

### 8. Java comparison

Java walked the same road in a different order:

| Era | Java | Rust equivalent |
|---|---|---|
| JDK 1.0–1.3 | `java.io`: blocking sockets, thread per connection | `std::net` + `std::thread` |
| JDK 1.4 (2002) | NIO `Selector`: readiness notification (epoll underneath on Linux) | `mio` |
| 2008 onward | Netty: event loops, pipelines, `ByteBuf` pools | Tokio's reactor + I/O types |
| JDK 8 (2014) | `CompletableFuture`, then reactive libraries (Reactor, RxJava) | futures combinators (Chapter 12.2) |
| JDK 21 (2023) | **Virtual threads** (JEP 444): blocking code, M:N scheduled | No equivalent in std (Chapter 12.6) |

Meridian's API gateway (400K requests/s at peak, Part I) runs on Netty, which is exactly an epoll event loop with
per-connection state objects (Netty's `Channel` and handler pipeline) that its authors wrote by hand. Java then took the
opposite turn from Rust: instead of making event-driven code easier to *write* (`async`/`await`), JDK 21 made blocking
code cheaper to *run*, by giving the JVM its own green threads. Chapter 12.6 compares the two bets properly.

> **Analogy limit.** "Tokio is Rust's Netty" is fair for the reactor and the event loop. It stops being true at the
> programming model: Netty handlers are callbacks you register, while Rust tasks are ordinary-looking functions that the
> compiler turns into state machines. And a Netty event loop runs *your* callbacks; a Rust executor calls `poll` on
> futures that decide for themselves when they're done.

### 9. Production scenario

**Sizing merchant-notify.** The Java service runs on Netty today: 24 pods, about 25,000 connections each at peak. The
Rust pilot (2026) aims for 100,000 connections per pod. The architecture review used this chapter's measurements for a
first-order budget (arithmetic from verified per-unit numbers, not a benchmark):

| Design at 100,000 connections per pod | Per-connection user-space memory | Total |
|---|---|---|
| Thread per connection | 2 MiB stack reserved (≈10 KiB resident when idle), plus arenas | 200 GB virtual; kernel limits long before |
| Event loop, state in a slab (as `ch01-04`) | tens of bytes + protocol state | a few MB + protocol state |
| Async task per connection (Chapter 12.6 measures ≈250 B per idle task, before protocol buffers) | ≈250 B + buffers | ≈25 MB + buffers |

The decisive line isn't memory; it's the kernel limits in §6. At 100,000 connections a thread design fails on
`vm.max_map_count` and `pids.max` before RAM matters. The remaining budget question, buffers, turned out to be the one
the team got wrong first (Chapter 12.3's production scenario).

### 10. Failure scenario

**The 2019 reconnect storm.** merchant-notify's first version (Java, 2019) used blocking sockets and a thread per
connection. Pods ran at about 8,000 connections each. A firmware update for one brand of point-of-sale terminal
introduced a bug: after any network blip, the terminal opened a new connection *without closing the old one*. During an
ISP incident in one region, connections on the affected pods climbed past 30,000 within minutes, and then every pod in
the region crash-looped with:

```text
java.lang.OutOfMemoryError: unable to create native thread: possibly out of memory or process/resource limits reached
```

The heap dashboards showed 40% usage, so the first hour went to the wrong theory. The actual limit was
`vm.max_map_count` (65,530 mappings at two per thread stack, so roughly 32,000 threads). Each crash dropped every
connection on the pod, and every terminal reconnected, some twice, to the surviving pods. It was a textbook metastable
failure: the recovery traffic was worse than the original.

The fixes were in three layers, and only one of them was "use an event loop":

1. **Immediate:** per-client-IP connection caps and idle-connection reaping (a connection with no heartbeat for 90 s is
   closed). The terminal bug had created connections nobody used.
2. **Architecture (2020):** migrate to Netty. Idle connections now cost a `Channel` object and kernel buffers, not a
   thread.
3. **Admission control:** a hard per-pod connection limit that refuses new connections cleanly (with a retry-after
   hint) instead of failing in `pthread_create`. Part XIII builds exactly this into Ferrite v2.

The lesson for an architect: an event loop raises the ceiling by two or three orders of magnitude, but **every design
has a ceiling**. The ones that survive incidents are the ones that know where their ceiling is and refuse work cleanly
before reaching it.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XII).*

1. What does a blocked `read(2)` cost the system, in memory and in kernel resources? Why is the thread's stack "where
   the connection's state lives"?
2. In `ch01-01`, 500 idle threads showed 3 GB of virtual memory and 7 MB resident. Explain both numbers, including the
   jump in the first 100 threads.
3. Compare `select`, `poll`, and `epoll`. Why is the cost of `epoll_wait` proportional to ready descriptors rather than
   registered ones?
4. What's the difference between level-triggered and edge-triggered epoll? What bug do you get if you use
   edge-triggered mode and read only part of the available data?
5. Readiness-based vs completion-based I/O: why do io_uring-based Rust runtimes use owned buffers instead of `&mut [u8]`?
6. Name three kernel limits a thread-per-connection server can hit before it runs out of RAM, and how each shows up.
7. Why did Rust remove green threads before 1.0, and what did it put in the language instead?
8. When is thread-per-connection the better design? Give a concrete workload.

### 12. Exercises

- **Beginner.** Modify `ch01-02` to use `set_read_timeout` with a blocking socket instead of busy polling. How many
  system calls does it make now? What's the latency cost of choosing the timeout badly?
- **Intermediate.** Extend `ch01-04`'s event loop into a line-based protocol: clients send `ECHO <text>\n`, possibly
  split across several TCP segments. Where do you store a half-received line? Measure the per-connection memory with
  10,000 connections, each holding a partial line.
- **Advanced.** Rewrite `ch01-03` to register interest in `EPOLLOUT` as well, and demonstrate the classic event-loop
  mistake: staying registered for writability on a socket with nothing to write. What does `epoll_wait` return, and what
  does it do to CPU usage?
- **Systems.** On a Linux machine (not verified here: requires a local toolchain and root for some settings), run
  `ch01-01` without the sandbox. Record the connection count at which it fails, then check
  `cat /proc/sys/vm/max_map_count`, `ulimit -u`, and the cgroup's `pids.max`. Which limit won? Raise it and find the next
  one.
- **Architecture.** A partner asks Meridian for a server-sent-events stream per API key, with an estimated 2 million
  concurrent subscribers. Write a one-page capacity plan: connections per pod, memory per connection (kernel and user),
  file descriptors, and the admission-control policy.

### 13. Debugging exercise

An engineer converts the blocking echo server to mio, and under load some clients stop receiving echoes. No error is
logged. The read path:

```rust,ignore
} else {
    let stream = &mut conns[ev.token().0];
    let mut buf = [0u8; 64];
    match stream.read(&mut buf) {
        Ok(0) => {}
        Ok(n) => stream.write_all(&buf[..n])?,
        Err(e) if e.kind() == ErrorKind::WouldBlock => {}
        Err(e) => return Err(e),
    }
}
```

1. Clients that send more than 64 bytes at once are the ones that hang. Why? (Which epoll mode does mio use, and what
   does this code fail to do?)
2. Fix it. Then find the *second* latent bug: what does `write_all` do on a nonblocking socket whose send buffer is full?
3. Why would the same bug never appear with a blocking thread per connection?

### 14. Design exercise

**merchant-notify's admission policy.** Using this chapter's numbers, design the connection admission and shedding
policy for a Rust merchant-notify pod targeting 100,000 connections:

- What hard limit per pod, and what happens to the 100,001st connection (reject at accept? accept and close with a
  retry hint?)
- Per-client-IP and per-merchant limits, to contain the 2019 terminal bug class.
- Idle reaping: heartbeat interval, timeout, and the cost of heartbeats themselves at 100,000 connections.
- Which OS limits (`RLIMIT_NOFILE`, `vm.max_map_count`, socket buffer sizes) you set in the container spec, and why.

Keep the answer; Part XIII's Ferrite v2 project implements the connection limit and graceful shedding.
