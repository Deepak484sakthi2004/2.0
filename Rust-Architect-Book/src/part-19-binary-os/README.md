# Part XIX — Binary, Linker, and OS

> **Part question:** *After rustc writes its last object file, what turns it into a running program, what does the
> operating system give that program, and which of those mechanisms decide where it runs, how fast it starts, how much
> memory it really uses, and what you can learn when it dies?*

Part XVIII ended at an object file. This Part follows it the rest of the way down the brief's chain:

```text
Rust source → machine code → executable → process → virtual address space → physical memory → CPU execution
             (Part XVIII)   (19.1–19.3)  (19.4)     (19.5)                    (19.5)            (19.6, and Part XX)
```

It's the most operational Part of the book. Almost every section ends at something you'll meet in production: a glibc
version error at 02:00, a backtrace that shows only addresses, a flame graph that blames the wrong function, a
container whose RSS never goes down, a `SIGBUS` at log-rotation time, a pod that can't bind its port after a restart.

## How this Part gets its evidence

The Rust Playground runs your program in a Linux container (Ubuntu 24.04, glibc 2.39, kernel 7.0 on AWS, 4 vCPUs, a
512 MiB cgroup), and that container also has **`rustc`, `gcc`, and GNU binutils** (`readelf`, `objdump`, `nm`,
`strip`, `objcopy`, `addr2line`, `dwp`). So most listings in this Part are programs that build *other* programs with
the Playground's own compiler and then inspect them, or inspect themselves while running (`/proc/self/maps`,
`getauxval`, `getrusage`), or trace their own children with `ptrace(2)`. Every `readelf`, `objdump`, `nm`, and trace
output quoted in the chapters is real and was produced by a listing in `listings/part-19/`.

What isn't available there is labeled with the command to run locally: `strace`, `perf`, `ldd` on other machines, the
musl target (its `std` isn't installed on the Playground), and Windows and macOS tools for PE and Mach-O. Timings are
from one run on a shared machine and are labeled as noisy; where a second run gave different numbers, the chapters say
so.

Two listings use unstable compiler flags (`-Z relax-elf-relocations`, `-Z plt`, `--print target-spec-json`) by setting
`RUSTC_BOOTSTRAP=1` for the Playground's stable compiler. That is for inspection only; it's labeled [VERSION] where
it appears, and nothing in a production build should depend on it.

## Chapter map

```text
19.1 Object Files, Symbols, Relocations   sections with holes; PLT32 vs GOTPCREL; what the linker fills and what it
                                          leaves for the loader (677 relocations); relaxation and the flag that
                                          enables it; v0 symbols; allocator shims; strip levels; build-ids and why a
                                          rebuild didn't match its own debug file
      │
19.2 Static and Dynamic Linking           what Rust links statically (all Rust code) and dynamically (glibc); the
                                          glibc floor and why weak references don't lower it; the loader's search;
                                          LD_PRELOAD interposition; cdylib exports; libstd.so and proc macros as dlopen'd
                                          shared libraries; rust-lld vs GNU ld; musl
      │
19.3 Executable Formats                   an ELF parser you can read; segments vs sections; 5.1 MB on disk, 457 KB
                                          mapped; unwind tables (CIE, FDE, personality, LSDA) and landing pads for
                                          extern "C" vs "C-unwind"; debug-info policies measured; frame pointers;
                                          PE, Mach-O, and a WebAssembly module read by hand
      │
19.4 From Executable to Process           the auxiliary vector; every system call before main (62); _start →
                                          __libc_start_main → lang_start → your main; what std sets up and why; ASLR
                                          observed; exit statuses decoded (1, 101, 127, 134, 135, 137, 139); pipefail
      │
19.5 Virtual Memory, Pages, and mmap      every mapping of a real process; page faults measured; huge pages; thread
                                          stacks reserved vs used; why freed memory stays resident (glibc,
                                          fragmentation); mmap vs read for logstat's input; SIGBUS; canonical addresses
      │
19.6 Syscalls, FDs, Processes, Threads    the syscall ABI; 540 ns per call vs 25 ns through the vDSO; descriptor
                                          tables, limits, and inheritance; fork and copy-on-write; posix_spawn;
                                          clone flags that make a thread; when a Mutex calls futex
      │
Part XIX Review                           a release-artifact audit: the PR's build fails 7 of 9 checks, the fixed build
                                          passes all; interview mode
```

## What you'll be able to do after Part XIX

- Read any Linux binary's symbols, relocations, segments, dynamic dependencies, and version needs, and explain what
  each means for where and how it runs.
- Choose a linking strategy (dynamic glibc, static glibc, musl, `cdylib`) per artifact, with the portability and
  patching consequences stated.
- Design a debug-information and symbolization pipeline that keeps production binaries small without losing the
  ability to diagnose a crash.
- Explain everything that happens before `main` and decode every exit status a supervisor reports.
- Size a service's memory from measurements (RSS, PSS, allocator retention, page cache) instead of guesses, and choose
  between `read` and `mmap` for input with the failure modes in view.
- Reason about system-call costs, descriptor limits, process creation, and lock contention from the kernel's side.

## Listings

`listings/part-19/`: 29 files, 31 checks, all verified on rustc 1.98.1 (edition 2024). Every listing that builds a
program inside the container fails if that build fails, so an "ok" can't hide a broken experiment.
