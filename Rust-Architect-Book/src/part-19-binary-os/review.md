# Part XIX Review — The Release Audit & Interview Mode

> Consolidate Part XIX, then use it: review a pull request that changes how a payments service is built and shipped,
> predict what's wrong from the diff alone, then run an audit of both binaries and explain every failing check. Then
> answer senior-level questions without notes. Answers are in **Appendix A, Part XIX**.

---

## Part XIX on one page

```text
 OBJECT FILE     sections with holes + symbols (defined T/t, undefined U) + relocations
                 same crate → PLT32 → direct call; other crate/library → GOTPCREL → GOT slot
 LINKER          (rust-lld by default since 1.90) resolves symbols, lays out sections, --gc-sections,
                 applies what it can; RELATIVE/GLOB_DAT left for the loader (677 in a small program);
                 relaxation (GOTPCRELX) only if the compiler asked for it (rustc: -Z relax-elf-relocations)
 EXECUTABLE      ELF: segments (loader's view: LOAD r / r-x / rw RELRO / rw, TLS, GNU_STACK, EH_FRAME, NOTE)
                 vs sections (tools' view); only LOAD bytes are ever mapped (457 KB of a 5.1 MB debug file);
                 .eh_frame (CIE with personality + FDE with LSDA) drives unwinding; debug info never mapped
 LINKING POLICY  Rust code always static; glibc dynamic on -gnu (floor = newest MANDATORY version need,
                 weak refs don't help); musl fully static; cdylib exports only #[no_mangle]
 PROCESS         execve → kernel maps segments + ld.so + vDSO, builds stack (argv, envp, auxv) → ld.so loads,
                 relocates, RELRO-protects → _start (aligns rsp to 16) → __libc_start_main → main → lang_start
                 (fds 0-2, SIGPIPE ignored, overflow handler on sigaltstack) → catch_unwind(your main)
 EXIT            0 / 1 (Err from main) / 101 (panic) / 127 (not found: program or library) / 134 (SIGABRT: abort,
                 stack overflow) / 135 (SIGBUS) / 137 (SIGKILL: OOM) / 139 (SIGSEGV: unsafe/FFI)
 MEMORY          reserve ≠ resident; first touch = minor fault; MADV_DONTNEED drops pages; THP on request;
                 freed ≠ returned (glibc heap, fragmentation); mmap = page cache, zero-copy, SIGBUS if the file shrinks
 KERNEL API      syscall ≈ 540 ns here, vDSO clock ≈ 25 ns; fd table (lowest free number, soft/hard limits,
                 CLOEXEC or inherited); fork = COW; std spawns with posix_spawn (CLONE_VM|CLONE_VFORK);
                 a thread = clone(VM|FS|FILES|SIGHAND|THREAD|...); Mutex → futex only under contention
```

## Ten ideas to carry forward

1. **An object file is code with holes.** Symbols name the holes, relocations say how to fill them, and whatever the
   linker can't fill becomes work for the loader at every start.
2. **Two views of one file.** Segments are what runs; sections are what tools read. Stripping sections never changes
   what runs.
3. **Names are for humans at 02:00.** Keep them recoverable for every build that reached production: build-ids,
   separate debug files, reproducible paths (`--remap-path-prefix`).
4. **The build machine sets the glibc floor.** It's the newest *mandatory* version need, and `std`'s weak references
   still count. Build on the oldest glibc you support, or ship musl.
5. **Rust links Rust statically** because it has no stable ABI; a shared `libstd` is valid for exactly one compiler
   build. `pub` isn't export: a `cdylib`'s ABI is its `#[no_mangle]` items.
6. **Unwind tables are observability infrastructure.** Panics, backtraces, and profilers all walk stacks with them;
   frame pointers make profiling cheap; `extern "C"` call sites need no landing pads since 1.81.
7. **`main` is not the start.** Before it: 62 system calls, relocation, RELRO, `std`'s SIGPIPE and stack-overflow
   setup, and two `catch_unwind`s. After it: an exit status that encodes *how* the process ended.
8. **Virtual is free, resident costs.** Reservations (thread stacks, malloc arenas) are address space; pages become
   memory on first touch; freed memory may stay resident, by design.
9. **`mmap` trades a copy for a contract.** Zero-copy input from the page cache, paid for with `SIGBUS` if anyone else
   shrinks the file. That's why `Mmap::map` is `unsafe`.
10. **The kernel sees contention, not locking.** Syscalls cost hundreds of nanoseconds; batching, the vDSO, and
    uncontended locks keep a service out of the kernel.

## Capstone: the release audit

**Context.** Meridian's payments-core (Chapter 8.4's service) is moving its settlement workers from Kubernetes to a set
of hardened VMs in the PCI network segment, which run an older LTS release with **glibc 2.35**. At the same time, the
payments-core team adds a process watchdog that uses `pidfd_open` to monitor a helper process. An engineer opens a PR
titled *"Release pipeline: faster builds, better crash info"* that changes the release configuration:

```toml
# .cargo/config.toml, as proposed in the PR (reconstructed; not verified as a Cargo build here)
[build]
rustflags = [
  "-C", "target-cpu=native",           # "CI runners are fast; use every instruction they have"
  "-C", "relocation-model=static",     # "PIE is slower"
  "-C", "link-arg=-Wl,-z,lazy",        # "faster startup: resolve symbols on first call"
  "-C", "link-arg=-Wl,-z,execstack",   # copied from the old C JIT library's build script
  "-C", "link-arg=-Wl,-rpath,/home/ci/runner/_work/payments-core/target/release/deps",
]

[profile.release]
debug = 2                              # "so crash reports have line numbers"
```

and the watchdog's code declares the glibc wrapper directly:

```rust,ignore
// From the PR (listing review-01-release-audit.rs builds a reduced version of it as PR_SRC).
unsafe extern "C" { fn pidfd_open(pid: i32, flags: u32) -> i32; } // glibc wrapper, new in glibc 2.36
```

### Part A: predict from the diff

Before running anything, write down for each line of the PR what it changes in the binary, and whether that change is
harmful for this deployment (VMs with glibc 2.35, mixed CPU generations across the fleet, a crash pipeline that keys on
build-ids, and a security review for the PCI segment). Rank the problems by blast radius.

### Part B: run the audit

Listing `review-01-release-audit.rs` builds a reduced version of the service twice with the Playground's compiler:
once with the PR's flags and code, once with the fixed configuration (default flags plus `-g`, debug information split
out with `objcopy --only-keep-debug` and a `.gnu_debuglink`, and `pidfd_open` called as a raw system call). Then it
audits both binaries with nine checks, each built from a tool used in this Part:

```text
PR binary runs:    score 25159680 pidfd true
fixed binary runs: score 25159680 pidfd true
check                                  PR                                                      fixed  
PIE (ASLR for the executable)          FAIL   EXEC (Executable file)                           pass   DYN (Position-Independent Executable file)
full RELRO (GNU_RELRO + BIND_NOW)      FAIL   RELRO true, BIND_NOW false                       pass   RELRO true, BIND_NOW true
non-executable stack                   FAIL   GNU_STACK RWE                                    pass   GNU_STACK RW
no absolute RPATH/RUNPATH              FAIL   /home/ci/runner/_work/payments-core/target/release/deps pass   none
NEEDED within allowlist                pass   libgcc_s.so.1 libc.so.6 ld-linux-x86-64.so.2     pass   libgcc_s.so.1 libc.so.6 ld-linux-x86-64.so.2
glibc floor <= 2.35 (version needs)    FAIL   GLIBC_2.36 (pidfd_open)                          pass   GLIBC_2.34 (__libc_start_main pthread_key_create)
x86-64 baseline (no AVX registers)     FAIL   129 instructions use ymm/zmm                     pass   0 instructions use ymm/zmm
build-id present                       pass   b68d8749a9e2                                     pass   180ca94daf71
debug info shipped separately          FAIL   4518664 bytes, .debug_info true, .gnu_debuglink false pass   457520 bytes, .debug_info false, .gnu_debuglink true
PR fails 7 of 9 checks; fixed fails 0
```

Both binaries run correctly *on the Playground*, which is the trap: every defect here is about where and how the
binary will run, not about what it computes.

### Part C: explain every failure

For each failing check, answer three questions: which chapter's mechanism explains it, what would happen in
production, and what the fix is. Then answer these:

1. The PR's glibc floor is 2.36. What would the VMs print at startup, and would *any* code run (Chapter 19.2)?
2. The fixed build passes the floor check with `GLIBC_2.34`. If the watchdog used `std::process::Command` to start the
   helper instead, the floor would be 2.39 even with the fix, because of `std`'s weak `pidfd` references. What is the
   *real* fix for a fleet on glibc 2.35, and why is the audit necessary but not sufficient?
3. "129 instructions use ymm/zmm" came from a loop LLVM vectorized for the CI runner's CPU. Which incident in this book
   did the same thing, and what would the symptom be on the VMs?
4. The PR's `debug = 2` was meant to improve crash reports. Explain why it *didn't* fail the "build-id present" check
   but did fail the debug-info check, and what the fixed pipeline does instead (Chapters 19.1 and 19.3).
5. The PR claims `relocation-model=static` and `-z lazy` make the service faster. What does each actually change, what
   does it cost, and how would you measure the claimed benefit (Part XX)?
6. Which of the nine checks would you make release-blocking, which advisory, and which are missing (hint: Chapter 19.6's
   close-on-exec audit, Chapter 19.3's frame pointers, a size budget)?

### Part D: the release gate

Design the release gate that runs this audit for every Rust artifact at Meridian: where it runs (CI, a registry
webhook, admission control), how targets are described (the fleet inventory of glibc versions and CPU levels), how
exceptions are approved and expire, and how the gate itself is tested (a known-bad binary like the PR's must keep
failing).

## Interview mode

Answer aloud, without notes, in two to four sentences each. Answers are in Appendix A (Part XIX).

### Linking and binaries

1. What does the linker do with a `call` to a function in the same crate, in `std`, and in `libc.so.6`?
2. Why does Rust link its own code statically but use glibc dynamically by default?
3. What is a glibc "version need", and why does a binary built on a newer glibc fail on an older one even if you
   didn't call any new function?
4. What does `-C strip=debuginfo` remove, what does `-C strip=symbols` remove, and what does each cost you?

### Process lifecycle

5. Name five things that happen between `execve` and your `main`.
6. What exit statuses do a panic, a stack overflow, and an OOM kill produce, and why?
7. What does `std` do with SIGPIPE, and how does that change a Rust CLI's behavior in a shell pipeline?

### Memory

8. Explain the difference between `VmSize` and RSS with a thread stack as the example.
9. Why can RSS stay high after a large collection is dropped? When does `malloc_trim` help and when doesn't it?
10. When would you use `mmap` for input, and when would you refuse to?

### Kernel interface

11. What does a system call cost, and how does the vDSO avoid the cost for `Instant::now()`?
12. Why does `std::process::Command` avoid `fork`, and what does it use instead?
13. When does a `std::sync::Mutex` enter the kernel?

### Architecture

14. Design the debug-information and symbolization policy for a fleet of Rust services.
15. Choose a linking strategy for (a) a Kubernetes service, (b) a CLI shipped to customers, (c) a library loaded into the
    JVM, and justify each.

## Looking ahead: Part XX

Part XIX kept labeling numbers "one run, noisy" and pointing forward: the cost of a GOT indirection, frame pointers'
runtime price, page-fault costs, huge pages and TLB misses, system-call overhead in containers, futex traffic under
contention. Part XX turns those hypotheses into measurements: how to benchmark without lying to yourself, how to profile
CPU and allocations, how caches, branch prediction, false sharing, and SIMD show up in real numbers, and how contention
and NUMA shape tail latency.
