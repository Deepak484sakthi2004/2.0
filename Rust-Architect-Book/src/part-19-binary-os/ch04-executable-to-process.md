# Chapter 19.4 — From Executable to Process

> **Where this sits:** Part XIX · Binary, Linker, and OS · chapter 4 of 6
> **Prerequisites:** Chapters 19.1–19.3 (relocations, the dynamic loader, ELF segments), Chapter 8.3 (panics, exit
> statuses from the Rust side), Chapter 3.5 (`process::exit` skips destructors), Chapter 11.1 (threads and stacks).
> **After this chapter you can:** narrate every step between `execve` and your `main`, with the system calls that
> prove it; read the auxiliary vector and say what each entry is for; name each frame between `_start` and user code;
> explain what `std` does before `main` and why; show address-space randomization in action; and decode any exit
> status a supervisor reports (1, 101, 127, 134, 135, 137, 139) into what happened.

---

## Pass 1 · User level — *Before `main`, and after it*

### 1. Problem

"The program starts at `main`" is a useful lie. By the time your `main` runs, the kernel has built a new address space,
the dynamic loader has opened files and applied hundreds of relocations, glibc has set up thread-local storage, and
`std` has changed signal dispositions and installed a stack-overflow detector. All of that takes time, can fail in its
own ways, and shapes behavior you'll see in production: why a CLI's startup cost matters when it's spawned two million
times a night, why a stack overflow is reported as `SIGABRT` rather than `SIGSEGV`, why a write to a closed pipe returns
an error instead of killing a Rust program.

The other end is just as practical. A process ends with an **exit status**, a 16-bit number the parent reads. Kubernetes,
systemd, shells, and CI systems all make decisions based on it. Knowing that 137 means "killed by SIGKILL", usually
the OOM killer, and 134 means "aborted", which for Rust includes stack overflows, turns an alert into a diagnosis.

### 2. Mental model

```text
 parent: fork/clone ──► child: execve("./playground", argv, envp)
                               │
 KERNEL                        ├─ read ELF + program headers; map LOAD segments (Ch. 19.3)
                               ├─ map the INTERP (ld-linux-x86-64.so.2) and the vDSO
                               ├─ build the initial stack: argc, argv[], envp[], auxv[], strings, 16 random bytes
                               └─ jump to the loader's entry point
 DYNAMIC LOADER (ld.so)        ├─ relocate itself; read /etc/ld.so.cache; open + mmap each NEEDED library
                               ├─ apply relocations (RELATIVE, GLOB_DAT: Ch. 19.1); set up TLS
                               ├─ mprotect the RELRO ranges read-only
                               └─ jump to the executable's e_entry = _start
 C RUNTIME                     _start ──► __libc_start_main ──► main (the C-ABI main rustc generated)
 RUST RUNTIME                  std::rt::lang_start::<()> ──► lang_start_internal
                               ├─ check fds 0/1/2 are open; ignore SIGPIPE
                               ├─ install SIGSEGV/SIGBUS handlers on an alternate stack (stack-overflow detection)
                               └─ catch_unwind(|| your main())
 YOUR CODE                     playground::main
 EXIT                          return → exit code from main's result; panic → 101; abort → SIGABRT; exit_group
```

### 3. Rust code

Listing `ch04-02-process-start.rs` reads what the kernel handed it. First the **auxiliary vector**: key/value pairs
the kernel places on the new stack after `argv` and `envp`, readable with `getauxval`. The listing also says which
mapping each address points into:

```text
--- auxiliary vector ---
AT_PHDR            0x63c2261f2040  r--p playground
AT_ENTRY           0x63c226217280  r-xp playground
AT_BASE            0x70caf5815000  r--p ld-linux-x86-64.so.2
AT_SYSINFO_EHDR    0x70caf5813000  r-xp [vdso]
AT_RANDOM          0x7ffcb0a4ae09  rw-p [stack]
AT_EXECFN          0x7ffcb0a4bfe0  rw-p [stack]
AT_PAGESZ                  0x1000  
AT_PHNUM                      0xc  
AT_SECURE                     0x0  
AT_EXECFN string: "target/debug/playground"
```

| Entry | What it's for |
|---|---|
| `AT_PHDR`, `AT_PHNUM` | where the kernel mapped the program headers: the loader needs them to find `DYNAMIC`, `TLS`, `RELRO` |
| `AT_ENTRY` | the executable's entry point (`_start`), where the loader jumps when it's done |
| `AT_BASE` | where the loader itself was mapped |
| `AT_SYSINFO_EHDR` | the **vDSO**: a small ELF image the kernel maps into every process (Chapter 19.6 uses it) |
| `AT_RANDOM` | 16 random bytes on the stack, for stack-protector canaries and pointer mangling |
| `AT_EXECFN` | the path used in `execve`, stored on the stack with `argv` and the environment strings |
| `AT_PAGESZ` | 4096: the page size (Chapter 19.5) |
| `AT_SECURE` | 1 for setuid programs: the loader then ignores `LD_LIBRARY_PATH` and `LD_PRELOAD` |

Then the listing captures a backtrace from inside `main` and prints the frames beneath it:

```text
0: playground::main
1: <fn() as core::ops::function::FnOnce<()>>::call_once
2: std::sys::backtrace::__rust_begin_short_backtrace::<fn(), ()>
3: std::rt::lang_start::<()>::{closure#0}
4: <&dyn core::ops::function::Fn<(), Output = i32> + core::panic::unwind_safe::RefUnwindSafe + core::marker::Sync as core::ops::function::FnOnce<()>>::call_once
5: std::panicking::catch_unwind::do_call::<&dyn core::ops::function::Fn<(), Output = i32> + core::panic::unwind_safe::RefUnwindSafe + core::marker::Sync, i32>
6: std::panicking::catch_unwind::<i32, &dyn core::ops::function::Fn<(), Output = i32> + core::panic::unwind_safe::RefUnwindSafe + core::marker::Sync>
7: std::panic::catch_unwind::<&dyn core::ops::function::Fn<(), Output = i32> + core::panic::unwind_safe::RefUnwindSafe + core::marker::Sync, i32>
8: std::rt::lang_start_internal::{closure#0}
9: std::panicking::catch_unwind::do_call::<std::rt::lang_start_internal::{closure#0}, isize>
10: std::panicking::catch_unwind::<isize, std::rt::lang_start_internal::{closure#0}>
11: std::panic::catch_unwind::<std::rt::lang_start_internal::{closure#0}, isize>
12: std::rt::lang_start_internal
13: std::rt::lang_start::<()>
14: main
15: <unknown>
16: __libc_start_main
17: _start
```

Read it bottom-up: `_start` (the ELF entry point, from glibc's start files) calls `__libc_start_main`, which (through
an internal glibc frame, `<unknown>` because the Playground's libc has no symbols for it) calls `main`, the C-ABI
function rustc generated. That calls the generic `lang_start::<()>`, which calls the non-generic
`lang_start_internal`, which runs your `main` inside **two** `catch_unwind`s. The frames name the two reasons: the
inner one (frames 4–7) catches a panic from your `main` to turn it into exit code 101; the outer one (9–11) protects
the runtime's own setup and teardown. `__rust_begin_short_backtrace` (frame 2) is a marker: when a panic prints a short
backtrace, frames below it are hidden.

The generated `main` itself is tiny:

```text
000000000002f040 <main>:
   2f040:	push   %rax
   2f041:	mov    %rsi,%rdx
   2f044:	mov    0x52665(%rip),%rax        # 816b0 <_DYNAMIC+0x478>
   2f04b:	mov    (%rax),%al
   2f04d:	movslq %edi,%rsi
   2f050:	lea    -0x6c47(%rip),%rdi        # 28410 <playground[8939561ac95f1c8b]::main>
   2f057:	xor    %ecx,%ecx
   2f059:	call   262a0 <std[6c98fd8553dbae28]::rt::lang_start::<()>>
   2f05e:	pop    %rcx
   2f05f:	ret
```

It passes your `main` as a function pointer (`rdi`), `argc` widened to 64 bits (`rsi`), `argv` (`rdx`), and a zero
(`ecx`, the SIGPIPE mode: 0 means the default, "ignore") to `lang_start`. The one-byte load through a GOT slot looks
odd; a scratch check of the slot's relocation showed it points at `__rustc_debug_gdb_scripts_section__`, a debug
build's `.debug_gdb_scripts` section that tells `gdb` to load Rust's pretty-printers. The load keeps the linker from
discarding the section [RUSTC].

And the very first instructions of the program, at `e_entry`, from glibc's start file `crt1.o`:

```text
0000000000025280 <_start>:
   25280:	endbr64
   25284:	xor    %ebp,%ebp
   25286:	mov    %rdx,%r9
   25289:	pop    %rsi
   2528a:	mov    %rsp,%rdx
   2528d:	and    $0xfffffffffffffff0,%rsp
   25291:	push   %rax
   25292:	push   %rsp
   25293:	xor    %r8d,%r8d
   25296:	xor    %ecx,%ecx
   25298:	lea    0x9da1(%rip),%rdi        # 2f040 <main>
   2529f:	call   *0x5c163(%rip)        # 81408 <__libc_start_main@GLIBC_2.34>
   252a5:	hlt
```

Line by line: `endbr64` marks a valid indirect-branch target (Intel CET) [CPU]. `xor %ebp,%ebp` zeroes the frame
pointer, which marks the **end of the frame-pointer chain** that profilers walk (Chapter 19.3). `rdx` holds a
finalizer from the loader, saved in `r9`. `pop %rsi` takes `argc` off the kernel-built stack, and `rsp` now points at
`argv`, which goes in `rdx`. Then `and $-16,%rsp` **aligns the stack to 16 bytes**: the System V ABI requires `rsp` to
be 16-byte aligned at every `call`, the rule Chapter 17.8 §5 used to explain why functions sometimes push a register
they don't need, and this is where the whole program's alignment is established. The two pushes keep that alignment
while passing the stack's end as an argument. Finally it calls `__libc_start_main` with `main` (the function above)
in `rdi`, through a GOT slot, like every call into a shared library (Chapter 19.1). `hlt` is never reached:
`__libc_start_main` doesn't return, it calls `exit`.

## Pass 2 · Systems level — *Every system call before `main`*

### 4. Under the hood

Listing `ch04-01-mini-strace.rs` is a small `strace` built on `ptrace(2)`: it re-executes itself in a "scenario" mode
as a traced child and records every system call, its decoded arguments, and its result. (`strace` itself isn't on the
Playground; the listing is about 200 lines.) For an empty `main` with a clean environment, the complete trace is 62
calls. Annotated:

```text
   brk() = 107963918278656                                   ── ld.so: where is the heap?
   mmap(len=8192 prot=3 flags=0x22) = 123564377731072        ── ld.so: scratch memory
   access() = -No such file or directory (os error 2)        ── /etc/ld.so.preload? no
   openat("/etc/ld.so.cache") = 3                            ── the loader's directory cache
   fstat() = 0
   mmap(len=10863 prot=1 flags=0x2) = 123564377718784
   close() = 0
   openat("/lib/x86_64-linux-gnu/libgcc_s.so.1") = 3         ── NEEDED #1 (the unwinder)
   read(fd=3) = 832                                          ── ELF header + program headers
   fstat() = 0
   mmap(len=185256 prot=1 flags=0x802) = 123564377530368     ── reserve the whole range, read-only
   mmap(len=147456 prot=5 flags=0x812) = 123564377546752     ── map .text r-x over it (MAP_FIXED)
   mmap(len=16384 prot=1 flags=0x812) = 123564377694208      ── r-- data
   mmap(len=8192 prot=3 flags=0x812) = 123564377710592       ── rw- data
   close() = 0
   openat("/lib/x86_64-linux-gnu/libc.so.6") = 3             ── NEEDED #2, same pattern
   ... (read, pread64 ×2, fstat, five mmaps, close)
   mmap(len=12288 prot=3 flags=0x22) = 123564375343104       ── loader bookkeeping, TLS block (inferred)
   arch_prctl(code=0x1002) = 0                               ── ARCH_SET_FS: point %fs at thread-local storage
   set_tid_address() = 43                                    ── glibc thread setup
   set_robust_list() = 0
   rseq() = 0                                                ── restartable sequences registration
   mprotect(len=16384 prot=1) = 0                            ── RELRO: make relocated data read-only
   mprotect(len=4096 prot=1) = 0                                (inferred: one per object, libc,
   mprotect(len=20480 prot=1) = 0                                libgcc_s, executable, loader)
   mprotect(len=8192 prot=1) = 0
   prlimit64(resource=3) = 0                                 ── RLIMIT_STACK
   munmap(len=10863) = 0                                     ── done with ld.so.cache
   poll(nfds=3) = 1                                          ── std: are fds 0, 1, 2 open?
   rt_sigaction(sig=13) = 0                                  ── std: SIGPIPE → ignore
   getrandom() = 8
   brk() = 107963918278656                                   ── first heap allocation
   brk() = 107963918413824
   openat("/proc/self/maps") = 3                             ── std: find the main thread's stack bounds
   prlimit64(resource=3) = 0
   fstat() = 0
   read(fd=3) = 1024 ... read(fd=3) = 737
   close() = 0
   sched_getaffinity() = 8
   rt_sigaction(sig=11) = 0                                  ── std: SIGSEGV handler ...
   sigaltstack() = 0
   mmap(len=12288 prot=3 flags=0x20022) = 123564377718784    ── ... on an alternate signal stack
   mprotect(len=4096 prot=0) = 0                                 with its own guard page
   sigaltstack() = 0
   gettid() = 43
   rt_sigaction(sig=11) = 0
   rt_sigaction(sig=7) = 0                                   ── SIGBUS handler
   rt_sigaction(sig=7) = 0
   sigaltstack() = 0                                         ── main returned: tear down
   munmap(len=12288) = 0
   exit_group() = 0
```

(Arguments trimmed; `openat` paths are decoded by the tracer; lines I annotated come from the listing's output as
printed, minus a few repeated `read`s.) The trace answers questions the earlier chapters raised:

- **The loader's mapping pattern.** For each library, one `mmap` reserves the whole address range read-only (`flags
  0x802`: `MAP_PRIVATE | MAP_DENYWRITE`), then `MAP_FIXED` mappings (`0x812`) overlay each segment with its own
  permissions: `prot=5` is `PROT_READ | PROT_EXEC`, `prot=3` read-write. That's Chapter 19.3's segment table executed.
- **RELRO in action.** The four `mprotect(..., prot=1)` calls come after relocation and before `main`: the GOT slots
  Chapter 19.1 read from memory were writable only during these few microseconds.
- **What `std` adds** before your `main` [LIB]: it polls fds 0–2 so that if the process was started with a closed
  stdin/stdout/stderr, it can open `/dev/null` in their place (otherwise the first file you open would silently become
  "stdout"); it **ignores SIGPIPE** so that writing to a closed pipe returns `EPIPE` instead of killing the process
  (Project L1 relied on that, and Chapter 19.6 verifies it); it reads `/proc/self/maps` to find the main thread's stack
  and its guard page; and it installs `SIGSEGV`/`SIGBUS` handlers running on a separate 12 KiB **alternate signal
  stack** with its own guard page. A stack overflow faults on the guard page; the handler, running on the alternate
  stack because the normal one is exhausted, recognizes the address, prints the "has overflowed its stack" message you
  saw in the Part IX interlude, and aborts.
- **One `getrandom` of 8 bytes** happens during startup; this trace alone doesn't show which component asked for it.

The environment matters more than you'd expect:

```text
== empty main(), cargo's environment: 134 syscalls, 40 failed openat
== empty main(), clean environment:   62 syscalls, 0 failed openat
```

cargo runs programs with a long `LD_LIBRARY_PATH`, and the loader tries every directory in it for every library: 40
failed `openat` calls, and the +100 µs startup difference Chapter 19.2 measured. Output adds what you'd predict from
Chapter 3.5 and Project L1:

```text
== hello: 63 syscalls, 1 write(1, ...)
== println100: 162 syscalls, 100 write(1, ...)
== bufwriter100: 63 syscalls, 1 write(1, ...)
```

`println!` goes through a line-buffered stdout, one `write` per line; `BufWriter` over a locked stdout makes one
`write` for all 100 lines.

**Address-space layout randomization.** Listing `ch04-03-aslr.rs` re-executes itself three times and prints where
things landed:

```text
randomize_va_space = 2
exe 0x60bcc2489000  libc 0x759182814b90  heap 0x60bce1226da0  stack 0x7ffff44b7450  vdso 0x75918296b000
exe 0x599759974000  libc 0x75d8edad5b90  heap 0x599771a3fda0  stack 0x7ffd41a9dd80  vdso 0x75d8edc2c000
exe 0x5b68dcb26000  libc 0x7a0ca7278b90  heap 0x5b68f229fda0  stack 0x7ffda410e400  vdso 0x7a0ca73cf000
```

Every region moves on every run: the executable (possible because it's a PIE, Chapter 19.1), the heap (which starts at
a random offset above the executable), the libraries and vDSO (the `mmap` area), and the stack. The low 12 bits of the
libc address (`b90`) never change: randomization works in whole pages. The listing then tries `setarch -R`, which asks
the kernel to disable randomization for one process via `personality(ADDR_NO_RANDOMIZE)`:

```text
setarch: failed to set personality to (null): Operation not permitted
```

The Playground's container sandbox blocks that system call argument (a seccomp filter [OS], unverified in detail), which
is itself a realistic finding: container runtimes restrict what processes may do to themselves, and debugging
techniques that work on a laptop can fail in a pod.

**How a process ends.** Listing `ch04-04-exit-status.rs` re-executes itself with six different endings and decodes
what the parent sees: the raw `wait` status, `ExitStatus::code()`, `ExitStatus::signal()`, and what a shell reports as
`$?`:

```text
ending         raw     code() signal()  sh $?  what the parent learns
ok             0x0    Some(0)     None      0  
exit3        0x300    Some(3)     None      3  
error       0x6500  Some(101)     None    101  note: run with `RUST_BACKTRACE=1` environment variable to di
abort         0x86       None  Some(6)    134  
overflow      0x86       None  Some(6)    134  fatal runtime error: stack overflow, aborting
sleep          0x9       None  Some(9)    137  
```

The raw status packs two cases into 16 bits [OS]. A normal exit stores the code in the high byte (`0x300` → 3,
`0x6500` → 101). A death by signal stores the signal number in the low 7 bits, plus bit `0x80` if a core dump was
requested (`0x86` = signal 6, `SIGABRT`, with the core-dump bit). Shells flatten both into one number by reporting
**128 + signal** for signal deaths, which is why the same abort is `Some(6)` to Rust and `134` to a shell. Three
Rust-specific rows:

- **`error`**: `unwrap()` on an `Err` in `main` panics, and `lang_start` turns a panic in `main` into exit code
  **101** [LIB]. (Returning `Err` from `main` instead gives exit code 1, Chapter 8.1.)
- **`abort`**: `std::process::abort()` raises `SIGABRT`: 134.
- **`overflow`**: a stack overflow is detected by `std`'s `SIGSEGV` handler, which prints its message and then
  **aborts**, so it's also 134, not the 139 (`SIGSEGV`) you might expect. A genuine 139 from a Rust program means a
  segfault that *wasn't* a guard-page hit: `unsafe` code, FFI, or a bug in a C dependency.

And the last row: `SIGKILL` (sent here by the parent, in production usually by the kernel's OOM killer when a cgroup
exceeds its memory limit, or by an orchestrator after a grace period) is **137**. Nothing in the process runs: no
destructors, no panic hook, no log line.

### 5. Memory

The initial stack is built by the kernel from the top of the stack region down: the strings (`AT_EXECFN` is one, at
`0x7ffcb0a4bfe0` in the listing, just below the top of the `[stack]` mapping), then the 16 `AT_RANDOM` bytes, then
the pointer arrays `envp[]`, `argv[]`, and `auxv[]`, then `argc`. The main thread's stack mapping starts at 132 KiB and
grows on demand up to `RLIMIT_STACK` (the `prlimit64(resource=3)` calls in the trace read it; 8 MiB is the common
default) [OS]. The heap starts above the executable's data after a random gap (in the first run above, the first heap
block is about 0x1ed9_dda0 bytes, roughly 500 MB, above the executable's base) and grows with `brk`. Everything else, including libraries, thread stacks (Chapter 11.1), and large
allocations, comes from `mmap` in the region below the stack.

A freshly started process is already partly resident: the `sh` process in this Part's environment probe reported
`VmRSS: 1476 kB` and `VmLib: 1752 kB`. Most of that is shared, file-backed library code.

### 6. CPU / OS

`execve` replaces the address space wholesale, so the new program shares nothing with the old one except open file
descriptors without close-on-exec (Chapter 19.6), the process ID, and a few attributes. The CPU-visible setup is small:
the kernel loads `rsp` with the new stack pointer and jumps to the loader; `arch_prctl(ARCH_SET_FS)` points the `%fs`
segment base at the thread control block, which is how every `thread_local!` access and `errno` finds its data with a
single `%fs`-relative load [CPU]. `rseq` registers a per-thread area that lets glibc read the current CPU number
without a system call.

Costs, from this Part's measurements (one run, shared machine, order of magnitude): ~0.5 ms per spawn-run-wait for a
static binary and ~0.9 ms for a dynamic one (Chapter 19.2), 62 system calls before `main` returns in the minimal case.

## Pass 3 · Architect level — *Startup and exit as interfaces*

### 7. Trade-offs

| Concern | Choice | Effect |
|---|---|---|
| Startup cost per process | dynamic glibc vs static (musl or crt-static) | ~0.35 ms of loader work saved per start here |
| | clean vs inherited environment | cargo-style `LD_LIBRARY_PATH` cost 72 extra system calls |
| | process per task vs long-lived worker | the only choice that removes startup cost entirely |
| Exit status design | `main` returning `Result` → 1; custom codes via `ExitCode` (Project L1) | lets supervisors distinguish "retry" from "don't" |
| Panic strategy | `unwind`: panic → 101 after destructors | `abort`: panic → 134, no destructors, possible core dump |
| Diagnosability of death | panic hook + logging (Chapter 8.3) | nothing runs on SIGKILL: rely on the orchestrator's reason field (`OOMKilled`) |

> **Why not have `main` start faster by skipping `std`'s setup?** You can (`#![no_main]` with your own C `main`), and
> for embedded targets people do. For a server you'd be giving up the SIGPIPE policy, stdio sanity, stack-overflow
> detection, and `catch_unwind` around `main`, to save microseconds that are a rounding error next to the dynamic
> loader. The measurable startup wins are static linking, a clean environment, and not starting a process at all.

### 8. Java comparison

A JVM start does everything above, then much more: the `java` launcher (a small ELF program) `dlopen`s `libjvm.so`,
which reserves the heap, starts GC and JIT compiler threads, loads and verifies hundreds of JDK classes, and only then
calls your `main`. Typical JVM startup is tens to hundreds of milliseconds depending on the application and JDK version
(order of magnitude, not measured here), which is why the Java ecosystem has invested in AppCDS (class-data sharing
archives), CRaC (checkpoint/restore of a warmed-up JVM), and GraalVM Native Image.

| | Rust binary | JVM |
|---|---|---|
| Work before `main` | loader + libc + `std` setup: 62 syscalls in the minimal case | loader + JVM init + class loading |
| Signal handlers installed | `SIGSEGV`/`SIGBUS` for stack-overflow detection only | `SIGSEGV` for implicit null checks and safepoint polling, plus others |
| Uncaught failure in `main` | panic → 101 | uncaught exception → exit code 1 |
| Stack overflow | message + abort (134) | `StackOverflowError`, catchable |
| OOM kill by the kernel | 137, nothing runs | 137, nothing runs (heap OOM *inside* the JVM is an `OutOfMemoryError` first) |

> **Analogy limit.** The JVM uses `SIGSEGV` as a *normal control-flow mechanism*: a null dereference in JIT-compiled
> code faults on purpose and the JVM's handler turns it into a `NullPointerException`. Rust's handler exists only to
> turn one specific fault (the guard page) into a clear message before aborting. Any other `SIGSEGV` in a Rust process
> is a genuine bug, and nothing converts it into a recoverable error.

### 9. Production scenario

**Two million starts a night.** Meridian's ledger statement export (the design exercise in Chapter 9.2: about 2
million statements overnight) was built as a shell-driven pipeline that invoked the export CLI (Chapter 3.5's CLI) once
per statement. With a dynamic build started from a CI-style environment, this Part's measurements suggest about
0.9–1.0 ms per start; with a static build and a clean environment, about 0.5 ms (one run, noisy). Over 2 million starts
that's roughly 15 minutes of CPU time saved by the build choice alone.

The team made that change first, because it was one line in the release profile and one in the job's environment.
The real fix came next: the CLI grew a `--batch` mode that reads statement IDs from stdin and exports them in one
long-lived process, with a `BufWriter` per output file (listing `ch04-01`'s 100-writes-vs-1 lesson). Startup cost went
from 2 million process starts to a few dozen worker processes, and the pipeline's wall-clock time was dominated by the
ledger queries again, which is where it belonged.

### 10. Failure scenario

**The exports that "succeeded".** The batch script's inner loop was:

```bash
export-cli --statement "$id" | gzip > "out/$id.csv.gz"
```

In a release that added a stricter currency check, the CLI panicked for statements containing a legacy currency code.
The script used `bash` without `set -o pipefail`. A pipeline's status is the status of its **last** command, `gzip`,
which happily compressed zero bytes and exited 0. The job reported success; 1,200 customers received empty statement
files the next morning. Listing `ch04-04` reproduces the mechanism with a panicking child:

```text
--- a failing command in a pipeline ---
without pipefail: 0
with pipefail:    101
```

What Meridian changed:

1. Every batch script starts with `set -euo pipefail`, enforced by a shell linter in CI.
2. The export CLI got an **exit-code contract** (Project L1's `ExitCode` pattern): 0 success, 1 invalid input
   (don't retry), 2 transient failure (retry), and panics stay 101 (a bug: page someone).
3. The on-call runbook gained a table, because statuses kept being misread in incident channels:

| Status | Meaning for a Rust program | First thing to check |
|---|---|---|
| 1 | `main` returned `Err` | the error message on stderr |
| 101 | a panic reached `main` | the panic message and backtrace |
| 127 | the shell couldn't find the program, **or** the loader couldn't find a library | `error while loading shared libraries` on stderr (Ch. 19.2) |
| 134 | `SIGABRT`: `abort()`, `panic = "abort"`, a double panic, or a **stack overflow** | stderr for "has overflowed its stack" |
| 135 | `SIGBUS` | memory-mapped files that shrank (Ch. 19.5) |
| 137 | `SIGKILL`: OOM killer or a forced termination | the orchestrator's termination reason; memory limits |
| 139 | `SIGSEGV` that isn't a guard-page hit | `unsafe` code, FFI, C dependencies; get a core dump |

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XIX).*

1. List, in order, what happens between a shell running `./svc` and the first line of `main`. Which steps are the
   kernel's, the loader's, glibc's, and `std`'s?
2. What is the auxiliary vector? Name three entries and who uses them.
3. Why does `lang_start_internal` wrap your `main` in `catch_unwind`? What would change if it didn't?
4. What does `std` do with SIGPIPE, and why is that the right default for a server but surprising for a CLI in a
   shell pipeline?
5. How does `std` detect a stack overflow, and why does the process end with `SIGABRT` rather than `SIGSEGV`?
6. Decode the raw wait statuses `0x300`, `0x6500`, `0x86`, and `0x9`.
7. Why can't ASLR be turned off inside the Playground's container, and what does that tell you about debugging in
   production containers?
8. Compare what happens before `main` in a Rust binary and in a JVM, and the startup options each ecosystem offers.

### 12. Exercises

- **Beginner.** Run listing `ch04-04` locally with `panic = "abort"` in a Cargo profile. Which rows change, and what
  does the `error` row report now?
- **Intermediate.** Add a `closed-stdout` scenario to listing `ch04-01` that closes fd 1 before `execve` in the child,
  and find the system calls where `std` notices and repairs it.
- **Advanced.** Write a `#![no_main]` program with `#[unsafe(no_mangle)] pub extern "C" fn main(argc: i32, argv:
  *const *const u8) -> i32` and trace it with listing `ch04-01`. Which of `std`'s setup calls disappear, and what
  behaviors do you lose (test a stack overflow and a write to a closed pipe)?
- **Systems.** Measure startup of a static and a dynamic build with `perf stat -r 1000` (not verifiable on the
  Playground). How much of the time is in the kernel's `execve` versus user space? How do page-cache effects change
  the first run versus later runs?
- **Architecture.** Design the exit-status contract and restart policy for a fleet of Rust batch jobs run by
  Kubernetes `Job`s: which statuses retry, with what backoff, which alert, and how you'd distinguish an OOM kill from
  a panic in dashboards.

### 13. Debugging exercise

A Rust service in Kubernetes restarts every few hours. The pod status shows `Last State: Terminated, Reason: Error,
Exit Code: 134`. There's no panic message in the logs, and memory usage is flat. The service runs request handlers on
a thread pool with `stack_size(256 * 1024)` set by a well-meaning performance change last month, and it parses JSON
from partners with a recursive-descent parser.

1. Which endings in listing `ch04-04`'s table produce 134? Which one fits these symptoms?
2. Why might the "has overflowed its stack" message be missing from the logs even though `std` prints it?
3. How would you confirm the hypothesis in production without reproducing the crash, and what would you change
   (Part IX interlude, Chapter 11.1)?

### 14. Design exercise

**Startup budget for a serverless deployment.** Meridian wants to run the fraud feature scorer on a serverless
platform where each cold start is billed and visible in latency. Using this chapter's mechanisms, design the artifact
(target, linking, allocator, binary size, what `main` does before it can serve), estimate the cold-start budget
component by component (kernel, loader, `std`, application initialization, model loading), say which parts you'd
measure first, and compare with what a JVM-based scorer would need (AppCDS, CRaC, or Native Image).
