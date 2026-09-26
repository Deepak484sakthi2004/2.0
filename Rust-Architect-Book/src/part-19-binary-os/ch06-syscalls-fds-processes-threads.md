# Chapter 19.6 — Syscalls, File Descriptors, Processes, and Threads

> **Where this sits:** Part XIX · Binary, Linker, and OS · chapter 6 of 6
> **Prerequisites:** Chapter 19.4 (the system calls before `main`, exit statuses), Chapter 19.5 (address spaces,
> copy-on-write), Chapter 11.1 (threads and stacks), Chapters 11.3 and 14.4 (mutexes and futexes), Chapter 12.1
> (`epoll`, raising `RLIMIT_NOFILE`).
> **After this chapter you can:** follow a `std` call down to the `syscall` instruction and say what each layer adds;
> estimate what a system call costs and when the vDSO avoids one; reason about file descriptors as a per-process table
> with limits and inheritance rules; explain why `std` spawns processes with `posix_spawn` instead of `fork`; read the
> `clone` flags that make a thread a thread; and show when a `Mutex` actually enters the kernel.

---

## Pass 1 · User level — *The narrow waist between Rust and the kernel*

### 1. Problem

Everything a program does besides computing goes through a **system call**: reading a file, writing a socket, asking
the time (sometimes), creating a thread, sleeping on a lock, starting a process. `std` is a safe, portable layer over a
few hundred of them, and most of the time you don't need to see through it. You do when:

- **Performance** depends on how many system calls a request makes (Chapter 19.4 counted one `write` per `println!`).
- **Limits** bite: the default soft limit on open files here is 1,024, and a gateway holding tens of thousands of
  connections needs far more.
- **Inheritance** leaks: a child process silently receives every file descriptor its parent didn't mark
  close-on-exec, including listening sockets.
- **Concurrency** is diagnosed: a `Mutex` is a user-space atomic until it isn't, and "why is my service making 500,000
  `futex` calls a second?" is a question about contention.

This chapter makes each of those concrete with the Playground's kernel (Linux 7.0 on AWS, inside a container sandbox).

### 2. Mental model

```text
 std::fs::File::open(path)                 safe API: Result<File, io::Error>, O_CLOEXEC always
   └─ libc::open(path, flags)              C wrapper: sets errno, returns -1 on error
        └─ syscall instruction             rax = 257 (openat), rdi, rsi, rdx, r10, r8, r9 = arguments
             └─ kernel entry                mode switch; container: seccomp filter runs; then the handler
                  └─ fd table lookup/insert  per-process table: small integer → open file description → inode

 per process:                                        shared or copied by clone()/fork():
   file descriptor table   0 1 2 3 ...  ─────►  open file descriptions (offset, flags) ─► files, sockets, pipes
   address space           (Ch. 19.5)
   signal handlers
   threads = tasks sharing all of the above (CLONE_VM | CLONE_FILES | CLONE_SIGHAND | CLONE_THREAD ...)

 not every "system" call enters the kernel:
   clock_gettime, gettimeofday, getcpu  → vDSO: kernel-provided code + data mapped into the process ([vdso], [vvar])
```

### 3. Rust code

Listing `ch06-01-syscall-cost.rs` asks for the process ID four ways, from the top of the ladder to the bottom. The
bottom rung is a `syscall` instruction written by hand:

```rust
#[inline(never)]
fn raw_getpid() -> i64 {
    let ret: i64;
    // SAFETY: getpid (syscall 39 on x86-64 Linux) takes no arguments, can't fail, and touches no memory.
    // The `syscall` instruction clobbers rcx and r11.
    unsafe {
        std::arch::asm!("syscall", inlateout("rax") 39i64 => ret, lateout("rcx") _, lateout("r11") _, options(nostack));
    }
    ret
}
```

All four agree, and they cost the same (release, best of 5 × 200,000 calls, one run on a shared machine: noisy):

```text
the same pid four ways: [14, 14, 14, 14]
std::process::id()             548.1 ns/call
libc::getpid()                 547.6 ns/call
libc::syscall(SYS_getpid)      538.3 ns/call
asm! syscall (rax = 39)        536.8 ns/call
Instant::now()                  55.7 ns/call
clock_gettime (vDSO)            24.5 ns/call
syscall(SYS_clock_gettime)     652.8 ns/call
vdso mapping: 76d0e1187000-76d0e1189000 r-xp 00000000 00:00 0                          [vdso]
```

Three conclusions. First, the layers above the `syscall` instruction are nearly free: `std` and libc add about 10 ns
to a 540 ns operation. Second, **a system call costs about half a microsecond here**. That number is high compared
with what's often quoted for bare-metal Linux (order of 100 ns, commonly cited, not measured here); inside a container
the kernel also runs a seccomp filter on every call, and CPU-vulnerability mitigations add to entry and exit [OS]. Third,
**the vDSO avoids the kernel entirely**: `clock_gettime` reads kernel-maintained data from the `[vvar]` pages and
computes the time in user space in 24.5 ns, 27× cheaper than forcing the same call through the kernel.
`Instant::now()` uses it (`Instant::now().elapsed()` above makes two clock reads: ~2 × 24.5 ns plus arithmetic).

## Pass 2 · Systems level — *File descriptors, processes, threads, futexes*

### 4. Under the hood

**What the `syscall` instruction does.** The calling convention is its own ABI: the system call number goes in `rax`,
up to six arguments in `rdi, rsi, rdx, r10, r8, r9` (`r10`, not `rcx`, because the instruction itself uses `rcx`), and
the result comes back in `rax`, with errors as negative values between −4095 and −1 [OS]. The CPU saves the return
address in `rcx` and the flags in `r11` and jumps to the kernel's entry point in privileged mode [CPU]; that's why
the `asm!` block above declares `rcx` and `r11` clobbered. libc's wrapper converts `-EINTR` and friends into `-1` plus
`errno`; `std` converts that into `io::Error` (whose `raw_os_error()` gives the number back) and retries calls that
were interrupted by a signal (`EINTR`) where retrying is correct [LIB].

**File descriptors.** Listing `ch06-02-file-descriptors.rs` opens files and sockets through `std` and through raw
`libc`, and checks the close-on-exec flag on each:

```text
open fds at start: 0 1 2 3
std File        fd 3 close-on-exec: true
std TcpListener fd 4 close-on-exec: true
libc::open      fd 5 close-on-exec: false
fds the child process sees: 0 1 2 5
```

(At start, fd 3 is the directory handle the listing itself uses to list `/proc/self/fd`.) A file descriptor is an index
into the process's table, and new descriptors take the **lowest free number**. `std` opens everything with
`O_CLOEXEC` / `SOCK_CLOEXEC` [LIB], atomically, so that a descriptor can never leak into a child even if another
thread spawns one between `open` and a later `fcntl`. Descriptors from C code that doesn't do this (`fd 5` here) are
**inherited by every child process**: the `sh` child sees fd 5 and nothing else of the parent's.

The limit and what hitting it looks like:

```text
RLIMIT_NOFILE soft 1024 hard 524288
with soft limit 16: opened 10 more files, then: Too many open files (os error 24) (kind TooManyOpenFiles, raw Some(24))
```

The **soft** limit (1,024 here, a common default) is what's enforced; a process may raise it up to the **hard**
limit (524,288 here) itself, which is what Chapter 12.1's C10K listing did. Hitting it gives `EMFILE`, which `std`
reports as `io::ErrorKind::TooManyOpenFiles`.

And a write to a pipe whose reader is gone:

```text
write to closed pipe: Broken pipe (os error 32) (kind BrokenPipe); the process is still running
```

The kernel sends `SIGPIPE` for this, whose default action kills the process. `std` set `SIGPIPE` to "ignore" before
`main` (Chapter 19.4's trace: `rt_sigaction(sig=13)`), so the write returns `EPIPE` instead, and `logstat` (Project L1)
could treat it as "the reader is done" rather than dying.

**Processes: `fork`, copy-on-write, and why `std` doesn't use `fork`.** Listing `ch06-03-processes-and-threads.rs`
allocates and touches 64 MiB, then calls `fork`. The child writes to the first 16 MiB and reads all of it:

```text
child:  wrote 16 MiB -> 4097 faults (copy-on-write); read all 64 MiB -> 1 faults; sum 20480
parent: child exited (0); parent still sees data[0] = 1
```

`fork` doesn't copy memory. It copies the page tables and marks every private page read-only in both processes. The
first write to a page by either side faults, and the kernel copies that one page: 4,097 faults for 4,096 pages written.
Reading shares the pages and costs nothing. The parent's data is untouched.

That makes `fork` cheap in memory but expensive in page-table work for large processes, and it has a worse problem in
**multithreaded** programs: only the calling thread exists in the child, so any lock another thread held at the moment
of `fork` (inside `malloc`, inside a logger) stays locked forever in the child [OS]. That's why the listing notes it's
single-threaded when it forks, and why `std::process::Command` avoids `fork` when it can. Listing `ch04-01`'s tracer
shows what `Command::new("/bin/true").status()` does in the parent:

```text
   mmap(len=36864 prot=3 flags=0x20022) = 136626545000448
   rt_sigprocmask() = 0
   clone3() = -Function not implemented (os error 38)
   clone(flags=0x4111) = 54
   munmap(len=36864) = 0
   rt_sigprocmask() = 0
   wait4() = 54
```

`0x4111` is `CLONE_VM | CLONE_VFORK | SIGCHLD`: the child **shares** the parent's address space (no page-table copy at
all), runs on a small 36 KiB stack the parent just mapped, and the parent is suspended until the child calls `execve`
(`CLONE_VFORK`). That's glibc's `posix_spawn`, which `std` uses on Linux when the `Command` configuration allows it
[LIB]. The first attempt, `clone3`, fails with `ENOSYS`: the container sandbox's seccomp filter rejects it (a common
container-runtime choice, so the runtime can inspect `clone` flags), and glibc falls back to `clone`. Seven system calls
more than an empty program, in total.

**Threads are tasks with everything shared.** The same tracer's `spawn + join` scenario records the flags `std`'s
thread spawn passes (via `pthread_create`), and listing `ch06-03` decodes them with the `libc` crate's constants:

```text
thread clone flags 0x3d0f00 = CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND | CLONE_THREAD | CLONE_SYSVSEM | CLONE_SETTLS | CLONE_PARENT_SETTID | CLONE_CHILD_CLEARTID
```

A thread is a task that shares the address space (`VM`), the current directory (`FS`), the descriptor table
(`FILES`), and signal handlers (`SIGHAND`); belongs to the same thread group, so it has the same PID (`THREAD`); gets
its own thread-local storage (`SETTLS`, the `%fs` base from Chapter 19.4); and has its TID written to a location that
the kernel clears and wakes with a futex when the thread exits (`CHILD_CLEARTID`), which is how `join` waits. Both
the parent and child sides of the spawn, trimmed:

```text
   [main ] mmap(len=2101248 prot=0 flags=0x20022) = 124681870946304      ── 2 MiB + 4 KiB stack, no access
   [main ] mprotect(len=2097152 prot=3) = 0                              ── all but the guard page: rw
   [main ] clone3() = -Function not implemented (os error 38)
   [main ] clone(flags=0x3d0f00) = 48
   [child] rseq() = 0
   [child] set_robust_list() = 0
   [child] mmap(len=134217728 prot=0 flags=0x22) = 124681736683520      ── glibc: this thread's malloc arena
   [child] munmap(len=23068672) = 0                                      ── trimmed to an aligned 64 MiB
   [child] munmap(len=44040192) = 0
   [child] mprotect(len=135168 prot=3) = 0
   [child] gettid() = 48
   [child] sigaltstack() = 0                                             ── std: alternate signal stack
   [child] mmap(len=12288 prot=3 flags=0x20022) = 124681870934016        ── for this thread's overflow handler
   [child] mprotect(len=4096 prot=0) = 0
   ...
   [child] madvise() = 0
   [main ] futex(op=9) = 0                                               ── join: wait for the child's exit
   [child] exit() = 0
   [main ] exit_group() = 0
```

(Trimmed; `flags=0x20022` includes `MAP_STACK`.) Chapter 19.5 explained the stack reservation and the 64 MiB arena
from the memory side; here they're the system calls that create them. The kernel lists the result as tasks:

```text
main thread: pid 13, tid 13
  task    13  comm "playground"
  task    44  comm "settlement-work"
  task    45  comm "settlement-work"
```

The threads were named `settlement-worker-0` and `settlement-worker-1` with `thread::Builder::name`. `std` passes the
name to the kernel, which keeps **15 characters** (the `comm` field is 16 bytes with the terminator) [OS], so both
show up identically in `top`, `ps -L`, and crash tools. Name threads so the first 15 characters are distinct.

**When a `Mutex` enters the kernel.** Chapters 11.3 and 14.4 described `std`'s `Mutex` as an atomic in user space
that falls back to the `futex` system call only to sleep and wake. The tracer counts `futex` calls for the same
lock/unlock loop with one thread and with four:

```text
== uncontended Mutex, 1 x 10,000 lock/unlock pairs: 1 futex calls [WAIT_BITSET x1]
== contended Mutex, 4 x 10,000 lock/unlock pairs: 311 futex calls [WAKE x208 WAIT_BITSET x103]
```

(Output reformatted with spaces.) With no contention, 10,000 lock/unlock pairs made **zero** `futex` calls; the one
call is `join` waiting for the thread to exit. With four threads fighting over the lock, 40,000 pairs made 311: a thread
calls `FUTEX_WAIT` only when the lock stays taken after a short spin, and the unlocking thread calls `FUTEX_WAKE` only
if someone is sleeping. The exact count varies from run to run (earlier runs with 4 × 20,000 gave 105 and 530) and is
distorted by tracing itself, since every traced system call stops the thread. The shape is the point: the kernel sees
contention, not locking.

### 5. Memory

A file descriptor costs the kernel a table slot and a reference to an open file description; a socket additionally has
send and receive buffers in kernel memory, sized by `SO_SNDBUF`/`SO_RCVBUF` and autotuning, which count against no
per-process limit you can see in RSS (Chapter 12.1's C10K listing measured user-space cost only) [OS]. Each thread
costs its reserved stack, a guard page, an alternate signal stack with its own guard page, and, with glibc, possibly a
malloc arena reservation (§4). `fork` shares memory copy-on-write but doubles the **commit charge**: under strict
overcommit accounting (`vm.overcommit_memory = 2`), forking a process with 10 GB of private memory can fail even if the
child would `exec` immediately, which is another reason to use `posix_spawn` [OS].

### 6. CPU / OS

A system call is a change of privilege level, not a context switch: the same thread continues in the kernel, and returns
[CPU]. The expensive part is everything around it (mitigations for speculative-execution vulnerabilities, seccomp in
containers, auditing when enabled) and the cache and TLB pollution from running kernel code, which the 540 ns figure
includes only partly. A `futex` *wait*, by contrast, **is** a context switch: the thread sleeps, the scheduler runs
something else, and a wake puts it back on a run queue, microseconds end to end (Chapter 11.7 measured 6.5–19.4 µs for
a channel round trip that involves wakeups).

That's the cost model behind three recurring design rules in this book: batch small writes (`BufWriter`: 100 `write`
calls down to 1 in Chapter 19.4), keep locks uncontended rather than merely short (Chapter 11.7), and read time
through the vDSO (`Instant::now()` does) rather than through anything that enters the kernel.

## Pass 3 · Architect level — *Budgets for calls, descriptors, and tasks*

### 7. Trade-offs

| Concern | Option | Gains | Costs |
|---|---|---|---|
| Many small I/O operations | one system call each | simple, lowest latency per item | ~0.5 µs each here, plus kernel work |
| | batching (`BufWriter`, `writev`, `io_uring`) | amortized entry cost | latency until flush; complexity (`io_uring`: Chapter 12.1) |
| Time | `Instant::now()` (vDSO) | 25–60 ns | monotonic only; wall-clock is `SystemTime` |
| Starting processes | `Command` (`posix_spawn` path) | no page-table copy, fork-safe with threads | limited child setup between fork and exec |
| | `fork` + custom setup | full control in the child | unsafe with threads; page-table copy; commit charge |
| Descriptor limits | raise soft limit to hard at startup | headroom for connection spikes | must still bound connections deliberately (Chapter 13.5) |
| Inheritance | `std` (always `CLOEXEC`) | nothing leaks | C libraries and raw `libc` calls may not follow it |
| Isolation | threads | shared memory, cheap communication | a crash or memory corruption takes all threads down |
| | processes | failure isolation, separate address spaces | IPC cost, per-process startup (Chapter 19.4) |

> **Why not just fork worker processes like a pre-fork web server?** It works for single-threaded programs that fork
> before creating any threads. A Rust service usually has a Tokio runtime or thread pools by the time it knows it needs
> workers, and forking then leaves the child with locks held by threads that don't exist. If you need process isolation,
> spawn a fresh executable (`Command`) and communicate over pipes or sockets, and accept the startup cost.

### 8. Java comparison

The JVM sits on the same system calls. `FileInputStream` and NIO channels wrap file descriptors; `System.nanoTime()`
reads the clock through the vDSO, like `Instant::now()`; `ProcessBuilder` on Linux launches children through a helper
using `posix_spawn` by default in modern JDKs (attributed; the `jdk.lang.Process.launchMechanism` property selects the
mechanism); and every Java platform thread is a kernel task created with `clone`, exactly as above. The differences
are mostly in who manages limits and blocking:

| | Rust | Java |
|---|---|---|
| Descriptor lifetime | `Drop` closes it deterministically | closed by `close()`/try-with-resources; otherwise at GC via cleaners (non-deterministic) |
| Close-on-exec | `std` always sets it | the JDK's launch helper closes descriptors above 2 in the child (attributed) |
| Blocking system calls | block the calling thread; async code must avoid them (Chapter 13.2) | platform threads block; virtual threads pin their carrier during some blocking calls, and the JDK compensates for file I/O (attributed) |
| Lock contention | `futex` via `std`'s `Mutex` | `futex` via HotSpot's parking; `synchronized` inflation under contention |
| Thread names in the kernel | `Builder::name`, truncated to 15 characters | named per JVM implementation (not verified here) |

> **Analogy limit.** A leaked `FileInputStream` in Java is eventually closed by the garbage collector, so a descriptor
> leak shows up as "too many open files under load, fine after a GC". In Rust there is no such backstop and none is
> needed: the descriptor is closed when its owner is dropped (Chapter 3.5). A Rust descriptor leak is either a value
> you deliberately kept (a `Vec<File>` that grows), `mem::forget`, or an inherited descriptor from a child process's
> point of view (§10).

### 9. Production scenario

**The gateway's descriptor budget.** Meridian's gateway handles 400K requests/s at peak across 55 pods (Part I), keeps
connection pools to about 120 upstreams, and holds client connections open with keep-alive. Each pod's worst case was
estimated from its connection limits (client connections + upstream pool sizes × upstreams + listening sockets + log
files + a margin), and the policy became:

1. At startup, raise `RLIMIT_NOFILE`'s soft limit to the hard limit (as Chapter 12.1's listing did), and set the hard
   limit in the pod spec so that the budget is explicit rather than inherited from a node's defaults.
2. Enforce the connection limit in the gateway itself (Chapter 13.5), well below the descriptor limit, so running out
   of descriptors is a bug signal, not a load signal.
3. Treat `EMFILE` from `accept` as a load-shedding event: log it at most once per second, pause accepting for a few
   milliseconds, and keep serving existing connections. An accept loop that retries immediately on `EMFILE` spins at
   100% CPU, because the pending connection stays in the listen queue and the listener stays readable.
4. Export the number of open descriptors (`/proc/self/fd` count) and the limit as metrics, and alert at 80%.

### 10. Failure scenario

**The restart that couldn't bind.** The gateway links a vendored C library that exposes metrics on port 9100 for a
legacy monitoring system. The library opens its listening socket with plain `socket()` and `bind()`, **without**
`SOCK_CLOEXEC`. On configuration reloads, the same library runs a notification hook with `system("/usr/local/bin/notify
...")`, which forks and execs a shell.

`notify` sometimes hung for minutes waiting on a slow endpoint. During that time it held, unknowingly, an inherited
copy of the listening socket on port 9100 (listing `ch06-02`: a descriptor without close-on-exec shows up in the child).
When a deployment restarted the gateway container's process, the new process's metrics library failed to bind port
9100 with `EADDRINUSE`, because the orphaned `notify` still held the socket open. The gateway's readiness check
included the metrics endpoint, so the pod never became ready, and the rollout stalled at one pod until the hung
`notify` processes were killed by hand.

What made it confusing: Rust's own sockets were fine (`std` sets `CLOEXEC`), `ss -lntp` showed the port owned by a
process called `notify`, and the problem appeared only when a reload and a restart overlapped. The fixes:

1. Patch the vendored library to create sockets with `SOCK_CLOEXEC`, and add a startup assertion in the gateway that
   walks `/proc/self/fd` and fails fast if any descriptor lacks `FD_CLOEXEC` (listing `ch06-02`'s `fcntl` check).
2. Replace `system()` with a `Command` spawned from Rust, which also gives a timeout and a kill on shutdown.
3. Take the legacy metrics endpoint out of the readiness check: readiness should reflect the ability to serve traffic.

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XIX).*

1. Trace `File::open` from `std` to the `syscall` instruction. What does each layer add?
2. Why did `getpid` cost ~540 ns here while `clock_gettime` cost 24.5 ns? What is the vDSO?
3. What is `O_CLOEXEC`, why does `std` set it atomically at `open`, and what happens to descriptors without it when a
   process spawns a child?
4. Explain copy-on-write after `fork`, using the listing's 4,097 faults. Why is `fork` dangerous in a multithreaded
   program?
5. What does `clone(flags=0x4111)` do, and why does `std::process::Command` prefer it to `fork`?
6. Which `clone` flags make a thread a thread? How does `join` learn that a thread has exited?
7. When does `std`'s `Mutex` make a system call? What would 500,000 `futex` calls per second tell you about a
   service?
8. How do file-descriptor lifetimes differ between Rust and Java, and how does that change what a descriptor leak
   looks like?

### 12. Exercises

- **Beginner.** Extend listing `ch06-02` to open 100 `TcpStream`s to a local listener and print the descriptor numbers.
  Close every other one and open 10 more: which numbers do they get, and why?
- **Intermediate.** Add a `pipe` scenario to listing `ch04-01` that writes 1 MiB through `std::io::pipe` between two
  threads, with and without a `BufWriter`. Count the `write` and `read` system calls and relate them to the pipe
  buffer size (`F_GETPIPE_SZ`).
- **Advanced.** Measure the cost of `thread::spawn` + `join` and of `Command::new("/bin/true").status()` on the
  Playground (release, best of N), and explain the difference using the traces in §4.
- **Systems.** On a bare-metal or VM Linux machine, run listing `ch06-01` and compare the system-call cost with the
  Playground's 540 ns. Then run it inside a container with the default seccomp profile, and with seccomp disabled
  (`--security-opt seccomp=unconfined`), and attribute the difference.
- **Architecture.** Design the process model for a Meridian service that must run untrusted, analyst-supplied rule
  plugins: threads in one process, a pool of worker processes, or one process per evaluation. Consider isolation,
  startup cost (Chapter 19.4), descriptor inheritance, resource limits (`RLIMIT_*`, cgroups), and how results come back.

### 13. Debugging exercise

A Rust ingestion service's CPU usage jumps to 100% on one core whenever traffic spikes, and throughput *drops* at the
same time. A profile shows the time in the accept loop and in the kernel. The code is:

```rust,ignore
// Sketch of the service's accept loop (not a listing).
loop {
    match listener.accept() {
        Ok((sock, _)) => spawn_handler(sock),
        Err(e) => {
            log::warn!("accept failed: {e}");
            continue;
        }
    }
}
```

The logs are full of `accept failed: Too many open files (os error 24)`.

1. Explain the spin: why does `accept` fail again immediately, and why does the listener stay readable?
2. Why does throughput drop instead of plateauing?
3. Fix it in two layers: what the loop should do on `TooManyOpenFiles`, and what should have prevented reaching the
   limit (§9).

### 14. Design exercise

**A system-call budget for Ferrite v2 (Project L5).** For Ferrite's async server, estimate the system calls per
request for `GET` and `SET` on a keep-alive connection: reads, writes, `epoll` waits, timer operations, and any
futex traffic between threads. Propose how to measure them (this chapter's tracer, or `strace -c` locally), set a
budget (for example, at most two system calls per request under pipelining), and list the design changes that would
reduce the count: read buffering, write coalescing, `writev`, response batching, and when `io_uring` would be worth
its complexity.
