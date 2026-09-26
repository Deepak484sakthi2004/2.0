# Part 19 report

Finished by a resuming writer: the first writer stalled on a network outage after creating 10 listings and no prose.
This writer verified and fixed those listings, added 19 more, and wrote everything else.

## SUMMARY.md lines

Replace the Part XIX draft block with:

```markdown
- [Part XIX Overview](part-19-binary-os/README.md)
  - [19.1 Object Files, Symbols, and Relocations](part-19-binary-os/ch01-object-files-symbols-relocations.md)
  - [19.2 Static and Dynamic Linking](part-19-binary-os/ch02-static-dynamic-linking.md)
  - [19.3 Executable Formats: ELF, PE, Mach-O](part-19-binary-os/ch03-executable-formats.md)
  - [19.4 From Executable to Process](part-19-binary-os/ch04-executable-to-process.md)
  - [19.5 Virtual Memory, Pages, and mmap](part-19-binary-os/ch05-virtual-memory-mmap.md)
  - [19.6 Syscalls, File Descriptors, Processes, and Threads](part-19-binary-os/ch06-syscalls-fds-processes-threads.md)
  - [Part XIX Review: The Release Audit & Interview Mode](part-19-binary-os/review.md)
```

Appendix entry, in Part order among the answer keys:

```markdown
  - [Part XIX Answers](appendix/answers-part-19.md)
```

## PROGRESS concepts

| Concept | Where | Full treatment planned |
|---|---|---|
| Object file = sections + symbols + relocations; one section per function (rustc); `lang_start::<()>` instance defined in the user crate | 19.1 | — |
| Relocation types PC32 / PLT32 / GOTPCREL / GOTPCRELX / RELATIVE / GLOB_DAT; 677 dynamic relocs in a small debug exe (67 GLOB_DAT, 2 JUMP_SLOT, 608 RELATIVE) | 19.1 | — |
| GOT slot read from the running process: `r--p` after RELRO, equals `dlsym(getpid)` | 19.1 | — |
| Relaxation: gcc emits GOTPCRELX (`addr32 call helper`); rustc 1.98.1 emits plain GOTPCREL so calls into std stay indirect; `-Z relax-elf-relocations=yes` → GOTPCRELX → `addr32 call _print`; `-Z plt=yes` → PLT32 → direct call [RUSTC][VERSION] | 19.1 | XX (measure) |
| Target spec: `plt-by-default: false`, `relro-level: full`, `position-independent-executables: true`, `default-uwtable: true`, `linker-flavor: gnu-lld-cc` | 19.1, 19.3 | — |
| Symbol kinds (593 t, 452 r, 283 T, 95 U, ...); v0 raw vs `nm -C`; `rust_eh_personality`, `DW.ref.rust_eh_personality` | 19.1 | — |
| Allocator shims in pseudo-crate `__rustc`: default `__rust_alloc` = `jmp __rdl_alloc`; with `#[global_allocator]` calls `<A as GlobalAlloc>::alloc`; `__rust_no_alloc_shim_is_unstable_v2` | 19.1 | XV.5 (not written) |
| Strip sizes: 4,927,280 full / 613,848 strip-debug / 459,696 strip-all; backtraces per level; `addr2line` offline; build-id survives strip | 19.1 | — |
| Build-id reproducibility: no `-g` same across dirs; `-g` differs (comp dir); `--remap-path-prefix` same | 19.1 | XXII (reproducible builds) |
| Rust code static, glibc dynamic; static-pie via `+crt-static` (368 KB vs 1.46 MB); spawn 877 vs 511 µs (second run 716 vs 406; one run, noisy) | 19.2 | — |
| LD_PRELOAD interposition (4242 in dynamic, real pid in static) | 19.2 | — |
| glibc floor = newest mandatory version need; `__libc_start_main@GLIBC_2.34`; std's weak `pidfd_spawnp`/`pidfd_getpid` create a mandatory `GLIBC_2.39` need (`Flags: none`, with rust-lld AND GNU ld); simulated loader error | 19.2 | — |
| Loader search incl. `glibc-hwcaps/x86-64-v4/v3/v2` subdirs; RUNPATH `$ORIGIN`; exit 127 for missing lib; exit 1 for missing version | 19.2 | — |
| cdylib exports only `#[no_mangle]` (1 dynamic symbol); dlopen/dlsym | 19.2 | XVI (not written) |
| `-C prefer-dynamic`: 5,344-byte exe + `libstd-<hash>.so`; proc macros = host dylibs dlopen'd by `librustc_driver` (`__rustc_proc_macro_decls_<hash>__`) | 19.2 | XXII (proc-macro policy) |
| rust-lld default (`.comment: Linker: LLD 22.1.8`), `-C linker-features=-lld` for GNU ld; both give BIND_NOW + GNU_RELRO; musl std not installed on the Playground (E0463) | 19.2 | — |
| Hand-written ELF64 parser (header, PHDRs, sections, .interp, DT_NEEDED) cross-checked with AT_PHDR/AT_ENTRY; 5.1 MB debug file, 457 KB mapped | 19.3 | — |
| Unwind tables: CIE "zPLR" w/ personality, FDE w/ LSDA, 12-byte `.gcc_except_table`; `extern "C"` callee → no landing pad (nounwind since 1.81); `C-unwind` → pad + `_Unwind_Resume`; `panic=abort` + `C-unwind` → pad runs drop glue then `panic_cannot_unwind` | 19.3 | — |
| Debug-info variants measured (d0 has std's 6 .debug sections; line-tables-only; split-debuginfo packed .dwp / unpacked .dwo; strip=debuginfo 462 KB; strip=symbols 353 KB); objcopy only-keep-debug + debuglink | 19.3 | — |
| Frame pointers: +`push rbp; mov rbp,rsp`, +6 bytes, same .eh_frame | 19.3 | XX (runtime cost) |
| wasm32 module read by hand (338 bytes: type/function/memory/global/export/code + custom sections) | 19.3 | XXV.4 |
| auxv entries and where they point; backtrace from `_start` to user main (two catch_unwinds); generated C `main`; `_start` aligns rsp to 16 and zeroes rbp | 19.4 | — |
| mini-strace (ptrace) of process start: 62 syscalls clean env vs 134 with cargo's env (40 failed openat); std init: poll(fds 0-2), SIGPIPE ignore, /proc/self/maps, sigaltstack + SIGSEGV/SIGBUS handlers | 19.4 | — |
| println 100 lines = 100 writes; BufWriter = 1 write | 19.4 | — |
| ASLR observed across 3 runs; `setarch -R` blocked by the sandbox (personality) | 19.4 | — |
| Exit statuses decoded: 0x300→3, 0x6500→101, 0x86 (SIGABRT+core)→134, stack overflow→134, SIGKILL→137, SIGBUS→135; pipefail demo | 19.4 | — |
| Address space tour: glibc per-thread arena (64 MiB aligned reservation), thread stack + guard page, mmap'd large Vec, vvar/vdso/vsyscall; canonical 47-bit addresses | 19.5 | — |
| Demand paging: 16,384 faults for 64 MiB; MADV_DONTNEED → zeros; THP madvise: 32 faults + 64 MiB AnonHugePages in one run, 2–4 MiB in others | 19.5 | XX.5 (cross-referenced) |
| Thread start: VmSize +67,600 KiB, RSS +16 KiB; 1 MiB frame → +1,024 KiB and 256 faults on entry (stack probes) | 19.5 | — |
| RSS after free (glibc): 128 MiB block returned; 1M small boxes → 78.5 MiB retained; malloc_trim → 2.2; every-64th survivor → 86.1 MiB even after trim | 19.5 | XX.4 (cross-referenced) |
| read_until vs fs::read vs mmap (44 MB): 13 / 29 / 5.6 ms; faults 6 / 10,730 / 672; RssAnon vs RssFile; glibc dynamic mmap threshold caveat (22 MB) | 19.5 | XXIII.1 |
| mmap + truncate → SIGBUS (status 135); why `Mmap::map` is unsafe | 19.5 | XXIII.1 |
| Red zone + kernel signal frames; alternate signal stack | 19.5 | — |
| Syscall ladder and cost: ~540 ns per getpid any layer; vDSO clock_gettime 24.5 ns vs 653 ns forced syscall; Instant::now 55.7 ns | 19.6 | XX |
| FDs: std sets CLOEXEC (File, TcpListener), raw libc::open inherited by child; RLIMIT_NOFILE 1024/524288; EMFILE → `TooManyOpenFiles`; EPIPE → `BrokenPipe` | 19.6 | XXI |
| fork COW: 4,097 faults for 4,096 pages written; std Command = clone(CLONE_VM|CLONE_VFORK|SIGCHLD) with 36 KiB stack; clone3 → ENOSYS in the sandbox | 19.6 | — |
| Thread clone flags 0x3d0f00 decoded; comm truncated to 15 chars; join via futex (CHILD_CLEARTID) | 19.6 | — |
| Mutex futex counts: 1×10,000 uncontended → 0 (1 is join); 4×10,000 contended → 311 (varies: 105, 530 in earlier runs) | 19.6 | — |

## Promises to later Parts

- **Part XX:** measure the GOT-indirection cost vs relaxation (19.1 advanced exercise); frame pointers' runtime cost
  (19.3); huge pages and TLB misses (19.5; XX.5 already has data); syscall cost in containers vs bare metal (19.6
  systems exercise).
- **Part XXI:** EMFILE handling in accept loops; descriptor budgets; TLS hardening of L3/L4 (unchanged from PROGRESS).
- **Part XXII:** reproducible builds (`--remap-path-prefix`, build-id comparison job, 19.1 design exercise); release
  gate / artifact audit in CI (Part XIX review Part D); proc-macro allowlist (19.2 Q7).
- **Part XXIII:** `mmap` policy for Ferrite's segment files vs WAL (19.5 design exercise), SIGBUS contract.
- **Part XVI (not written):** `cdylib` exported-symbol surface audit; `repr(C, u32)` etc. remain open there.

## Promises kept

- PROGRESS Part XIX: mmap input for logstat (19.5, measured, plus the SIGBUS failure mode); linking in depth (19.1,
  19.2); unwind tables `.eh_frame`/LSDA and backtrace symbolization (19.3, 19.1); exit statuses (19.4); freed memory
  vs RSS (19.5, with fragmentation); guard pages and stack probes (19.5, 19.4); musl static linking (19.2: discussed;
  unverifiable on the Playground, labeled with E0463 evidence); strace of thread spawn and futex syscalls (19.6 via the
  ptrace tracer); thread-stack virtual memory (19.5); canonical addresses and TBI/LAM (19.5, hardware parts labeled).
- Part XVIII promises: GOT-relative calls and relaxation (19.1, with the rustc flag that controls it); v0 symbols and
  demangling (19.1); `lang_start` wrapping `main` (19.4, full frame list + generated `main`); personality routine and
  unwind tables (19.3); allocator shims (19.1, disassembled); proc macros as dlopen'd host dylibs (19.2, LD_DEBUG
  trace); `split-debuginfo`/`strip` (19.3, measured).
- Part XVII promises: GOTPCREL and relaxation cross-referenced to 17.8 (19.1); stack frames, red zone, 16-byte
  alignment (19.4 `_start` disassembly, 19.5 red zone + signal frames).
- Part XX: page-fault cost (~2 µs, 20.4), THP mostly not granted (20.5), RSS vs live heap (20.4) cross-referenced in
  19.5 instead of re-measured.
- Chapter 11.1 "Chapter 19.5 goes deeper into virtual memory": kept. Chapter 2.1 "Part XIX covers linking in depth":
  kept. Project L1 omissions table "mmap input: Part XIX": kept.
- Not kept: nothing in the PROGRESS Part XIX bullet was left open.

## Meridian facts introduced

| System | Facts established | Where |
|---|---|---|
| Rust symbol pipeline | release profile `debug = "line-tables-only"`; `objcopy --only-keep-debug` in CI; debug files uploaded to an internal symbol server keyed by build-id; stripped binaries shipped | 19.1 §9 |
| Market-data ingest build-id mismatch | panic with stripped backtrace; upload had failed; rebuild on a different runner had a different build-id (paths in DWARF); fixes: `--remap-path-prefix` for workspace and `$CARGO_HOME`, upload as release gate, weekly rebuild-compare job, "never symbolize with a rebuild" | 19.1 §10 |
| Linking policy per artifact | edge services musl static + mimalloc FROM scratch (reasons written down); payments-core glibc built on oldest-fleet-glibc builder, distroless; fraud FFM cdylib built on oldest glibc, CI checks `nm -D --defined-only` against the header; CLIs musl; never `prefer-dynamic` or `LD_LIBRARY_PATH` in production | 19.2 §9 |
| Settlement job glibc-floor incident | CI moved to glibc 2.39 images; settlement VMs on glibc 2.31; job failed at 02:00 with `GLIBC_2.34` / `GLIBC_2.39` not found; settlement 4 h late; fixes: pinned builder, CI floor audit, musl for batch jobs, staging OS parity | 19.2 §10 |
| Observability build policy | continuous profiler walks frame pointers fleet-wide; `strip = "debuginfo"`; `-C force-frame-pointers=yes` for services (cost measured on the gateway before rollout); `panic = "unwind"`, `extern "C"` at FFI unless design review approves `C-unwind` | 19.3 §9 |
| Fraud flame graph incident | frame-pointer profiler on a build without frame pointers blamed `memcpy`; a sprint wasted; real hot path the `HashMap<String, f64>` lookup (9.5); fixes: frame pointers, know prebuilt-std limits, profiler canary | 19.3 §10 |
| Statement export startup | ~2M CLI starts per night; static + clean env saves ~15 CPU-minutes (from one noisy run); real fix `--batch` mode long-lived workers | 19.4 §9 |
| Empty statements incident | `export-cli | gzip` without pipefail; a panic (legacy currency code) masked as success; 1,200 customers got empty files; fixes: `set -euo pipefail` lint, exit-code contract 0/1/2/101, runbook exit-status table | 19.4 §10 |
| Session cache "nightly leak" | 03:00 purge of ~1.2M sessions; RSS stayed near peak; 85% alert; fragmentation, not a leak; fixes: graph allocator in-use next to RSS, limit from peak + retention headroom, moved to mimalloc after a canary | 19.5 §9 |
| logstat sidecar SIGBUS | mmap fork of logstat; logrotate `copytruncate` at midnight → SIGBUS (135) crash loop; handler rejected; buffered reader restored; mmap only for exclusively owned files; logrotate `create` | 19.5 §10 |
| Gateway descriptor budget | soft limit raised to hard at startup, hard limit in pod spec; connection limit below fd limit; EMFILE → pause accepting, rate-limited log; fd count metric, alert at 80% | 19.6 §9 |
| Port 9100 inherited-socket incident | vendored C metrics library opened its listener without `SOCK_CLOEXEC`; `system()` notify hook inherited it; hung notify held port 9100; restarted gateway got EADDRINUSE; readiness stalled rollout; fixes: patch to SOCK_CLOEXEC, startup CLOEXEC assertion, `Command` instead of `system()`, metrics out of readiness | 19.6 §10 |
| payments-core release PR (review capstone) | PCI-segment VMs on glibc 2.35; watchdog uses pidfd_open; PR flags target-cpu=native, relocation-model=static, -z lazy, -z execstack, absolute CI rpath, debug = 2; audit: PR fails 7 of 9, fixed passes all | Part XIX review |

## Verification

- `listings/part-19/`: **29 files, 31 checks, all PASS** in a final full-folder run (rustc 1.98.1, edition 2024), with
  **zero** `Invoke-WebRequest` errors in the log (see the tooling note on false passes below). Final log saved to
  scratch `part-19/verify-final.txt`.
- Most listings build other programs inside the Playground container with `rustc`/`gcc` and inspect them with GNU
  binutils (`readelf`, `objdump`, `nm`, `strip`, `objcopy`, `addr2line`, `dwp`), or trace their own children with
  `ptrace` (`ch04-01`, a ~230-line mini-strace). Every listing that builds something asserts that the build succeeded
  (a `must()` helper), so an "ok" can't hide a failed experiment (the original `ch03-02` passed while its inner build
  failed; fixed).
- One listing (`ch01-06`) uses `RUSTC_BOOTSTRAP=1` to run unstable `-Z` flags on the stable compiler, labeled
  [VERSION] in the text as inspection-only. The "weak references still create a mandatory GLIBC_2.39 need, with both
  linkers" claim is backed by `ch02-01` (rust-lld) and `ch02-05` (rust-lld and GNU ld).
- Timings are one run each on the shared machine, labeled; `ch02-02`'s second run is quoted to show the spread.
- Unverifiable here and labeled: musl builds (std not installed: E0463 shown), PE/Mach-O tools, `strace`/`perf`,
  TBI/LAM hardware, sanitizing seccomp details, JVM startup numbers (order of magnitude), bare-metal syscall cost
  (commonly cited, not measured).
- All `rust` blocks (2) are verbatim substrings of verified listings; `rust,ignore` blocks are labeled excerpts or
  sketches. All six chapters have the 14 template sections and 4 pass headings.

## Word count

README 825 · 19.1 4,584 · 19.2 4,020 · 19.3 4,356 · 19.4 4,302 · 19.5 4,736 · 19.6 3,951 · review 1,800 ·
answers 6,052 → **34,626** words (`wc -w`, code included).

## Tooling notes

- **BUG in `tools/verify.ps1` (affects every Part): a Playground timeout is reported as PASS.** When the Playground
  answers `{"error":"The operation timed out: deadline has elapsed"}`, `Invoke-WebRequest` throws, `$resp` keeps the
  **previous check's** response, and the check is scored on stale data. In one full-folder run of this Part, four
  checks (`ch04-01`, `ch04-02`, `ch04-04`, `ch05-04`) timed out and were printed as PASS. Fix suggestion: wrap the
  request in `try { ... } catch { $pass = $false; $stderr = "request failed: $_" }` and reset `$resp = $null` at the
  top of each check (optionally retry once). Until fixed, grep verify logs for `Invoke-WebRequest :` and re-run those
  files individually. (This writer can't edit `tools/`.)
- **The Playground container has a toolchain and binutils.** `std::process::Command::new("sh")` can run `rustc`
  (stable 1.98.1, LLVM 22.1.8), `gcc` 13.3, `readelf`, `objdump`, `nm`, `strip`, `objcopy`, `addr2line`, `file`, `ldd`,
  `dwp`, `rust-lld`, and `/usr/bin/true`; targets installed: `x86_64-unknown-linux-gnu` and `wasm32-unknown-unknown`
  (no musl). No `strace`, `perf`, `gdb`, `valgrind`, `clang`. Writable `/tmp`.
- **`RUSTC_BOOTSTRAP=1 rustc -Z ...` works inside the container**, e.g. `-Z time-passes`, `-Z print-type-sizes`,
  `-Z relax-elf-relocations`, `--print target-spec-json`. This could have verified some things Parts XVIII and XX
  labeled "not verifiable (needs -Z flags)". Label such uses as inspection-only.
- `ptrace(PTRACE_TRACEME)` works (`ptrace_scope = 1`); `personality(ADDR_NO_RANDOMIZE)` (`setarch -R`) is blocked;
  `clone3` returns `ENOSYS` (glibc falls back to `clone`).
- Environment facts: Ubuntu 24.04.5, glibc 2.39, kernel 7.0.0-1011-aws, AMD EPYC 9R14 (4 vCPUs), cgroup
  `memory.max` 512 MiB and `pids.max` 512, `RLIMIT_NOFILE` 1024/524288, THP `madvise`, `overcommit_memory = 0`,
  `randomize_va_space = 2`.
- New lint on 1.98.1: `function_casts_as_integer` warns on `libc::getpid as usize`; cast through `*const ()` first.
- Rust string literals in listings: avoid `\.` in shell regexes (use `[.]`), since `"\."` is an invalid escape.
- A listing's inner builds should be asserted (`must()` helper) or the check can pass while the experiment failed.
