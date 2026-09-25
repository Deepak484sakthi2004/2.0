# Part 14 report

Status: **complete.** The original writer stopped on an API error while working on Chapter 14.5 (no transcript
survived). A second writer checked the five chapters on disk (all 14 template sections present, 14.5 ends cleanly),
re-verified every listing, and wrote the review, the answer key, one extra listing for the answer key, this report,
and corrected the README's listing counts.

## SUMMARY.md lines

Replace the Part XIV draft block with:

```markdown
- [Part XIV Overview](part-14-memory-model/README.md)
  - [14.1 Why Memory Ordering Exists: Store Buffers and Reordering](part-14-memory-model/ch01-why-ordering.md)
  - [14.2 Happens-Before](part-14-memory-model/ch02-happens-before.md)
  - [14.3 Relaxed, Acquire/Release, and SeqCst](part-14-memory-model/ch03-orderings.md)
  - [14.4 Compare-and-Swap, Fences, and Lock-Free Building Blocks](part-14-memory-model/ch04-cas-fences-lock-free.md)
  - [14.5 Rust Atomics vs Java volatile and VarHandle](part-14-memory-model/ch05-rust-vs-java.md)
  - [Part XIV Review: The SPSC Ring PR & Interview Mode](part-14-memory-model/review.md)
```

Appendix entry (in order among the answer keys):

```markdown
  - [Part XIV Answers](appendix/answers-part-14.md)
```

## PROGRESS concepts

| Concept | Where | Full treatment planned |
|---|---|---|
| Three reorderers (compiler, core, memory system); SC (Lamport 1979); coherence (per location) ≠ consistency | 14.1 | — |
| SB litmus on real x86 (AMD EPYC 9R14, 4 vCPUs): Relaxed 5,321 / Rel-Acq 14,901 / SeqCst 0 / SeqCst fence 0 of 200,000 (one run); MP weak outcome 0 of 200,000 on x86 | 14.1 | — |
| Reordering table x86-TSO / Armv8 / POWER (store→load only on x86; POWER not multi-copy atomic), with citations | 14.1 | XX.5 |
| Compiler hoisting a flag loop into `jmp` to itself (`&bool`, `static mut`) vs `Relaxed` load (verified asm); loop → `memcpy` with one progress store | 14.1 | XVII.7 |
| `compiler_fence` = `#MEMBARRIER` only; for signal handlers, never for threads | 14.1, 14.4 | XIX.6 |
| Happens-before = sb + sw; data race definition and UB [LANG]; C++20 model adopted (no consume) | 14.2 | — |
| Safe Rust can't race (E0499 on two scoped threads); `unsafe` race: debug lost 56% of increments, release collapsed loop to one `add`, Miri reports | 14.2 | XV |
| Rigorous `Relaxed` counter proof (RMW atomicity + spawn/join edges + coherence), Chapter 1.3's promise | 14.2 | — |
| Six std hb mechanisms (spawn/join, Mutex, channel, OnceLock, Barrier, Release/Acquire) checked by Miri (`miri-ok`) | 14.2 | XI |
| Miri race detection: vector clocks, retags count as accesses, one schedule per run; TSan (nightly, unverified here) | 14.2 | XV.6, XXII.7 |
| Read-read coherence measured: 61M scraper reads, 0 backwards | 14.2 | — |
| Language table: data races in Rust / C / C++ / Java / Go (Go 2022 model) | 14.2 | — |
| "Benign" racy hash cache: UB under Miri; atomic fix compiles to identical asm | 14.2 | — |
| Three questions (Q1 publication → Rel/Acq, Q2 RMW handover → AcqRel, Q3 cross-location agreement → SeqCst) | 14.3 | — |
| "Publish a buffer via a counter" counterexample: correct natively on x86, Miri data race; Release/Acquire fix | 14.3 | — |
| Miri weak-memory emulation: MP Relaxed 21/40, SB Relaxed 20/40, IRIW Rel/Acq 3/40, all 0 with the stronger orderings | 14.3 | — |
| Release sequences; P0668 (C++20 SeqCst revision) | 14.3 | — |
| Orderings → LLVM IR (`monotonic` … `seq_cst`; `unordered` unexposed) → x86 asm (only SeqCst store differs: `xchg`) | 14.3 | XVIII.6 |
| Ordering costs on x86 (one run): mov store 0.14 ns, xchg 2.08, lock xadd 2.07, SeqCst fence 1.98 | 14.3 | XX |
| AArch64 mappings (ldar/stlr/dmb, LSE vs LL/SC) — unverified, labeled | 14.3 | XX |
| `invalid_atomic_ordering` deny lint; run-time "there is no such thing as a relaxed fence" panic | 14.3 | — |
| ArcSwap for snapshot publication + reclamation (Miri-clean) | 14.3 | XXI, XXIII |
| RMW family and x86 lowering (`lock inc`, CAS loop for `fetch_or` with used result); CAS loop vs `try_update` vs `fetch_max` | 14.4 | — |
| `fetch_update` renamed `try_update` (deprecated on nightly 1.100.0, `update` added) [VERSION] | 14.4 | — |
| Spinlock (TTAS + backoff): Relaxed version keeps exclusion, Miri reports retag race | 14.4 | XI.3 |
| Seqlock with atomics + fences (Boehm 2012), naive UnsafeCell seqlock UB under Miri | 14.4 | — |
| Fence rules; `fence(SeqCst)` = `lock or dword ptr [rsp - 64], 0`; Arc drop = `lock dec` + `#MEMBARRIER`; MiniArc (Miri-clean) vs Relaxed refcount (Miri race) | 14.4 | XV |
| ABA replayed deterministically on an index free list; tagged head fix; no `AtomicU128` on baseline x86-64 (E0432) | 14.4 | — |
| Reclamation schemes table (EBR, hazard pointers, refcount, don't free); crossbeam-epoch Treiber stack Miri-clean under Tree Borrows only | 14.4 | XV.2 |
| False sharing measured (shared 13.64, adjacent 14.96, 64-aligned 4.62, CachePadded 5.76, thread-local 1.20 ns/inc; one run) | 14.4 | XX.5 |
| Spinning vs OS descheduling; `pause` latency varies by microarchitecture | 14.4 | XX.7 |
| Java ↔ Rust mapping table (VarHandle modes, volatile, AtomicX, fences, synchronized, final, LongAdder, DCL) | 14.5 | — |
| DCL hand-rolled (Miri-clean) vs Relaxed pre-Java-5 port (Miri race); `OnceLock`/`LazyLock` | 14.5 | — |
| `read_volatile`/`write_volatile` are not Java `volatile` (Miri race on the flag) | 14.5 | XVI (MMIO/FFI) |
| Striped counter (LongAdder) 8× faster than one AtomicU64 (one run); `sum()` not a snapshot | 14.5 | XX |
| Final-field semantics unnecessary in Rust; JMM vs C++20 differences (racy reads, OOTA, reentrancy, reclamation) | 14.5 | — |
| Testing tools: jcstress vs loom vs Miri vs TSan (loom/TSan unverified here) | 14.5 | XXII.7 |

## Promises to later Parts

- **Part XV (15.2):** Stacked vs Tree Borrows: `crossbeam-epoch` 0.9.20's `Local::element_of` (`container_of`-style
  cast) is UB under Stacked Borrows and accepted under Tree Borrows (14.4 §5 says "Chapter 15.2 covers the
  difference"). Also the obligations behind `unsafe impl Send/Sync` used throughout Part XIV, and `MaybeUninit` slots
  (the review's ring).
- **Part XI (11.3):** 14.4 and 14.5 cite Chapter 11.3 for `std::sync::Mutex` spinning briefly then sleeping on a futex,
  and for restructuring reentrant Java `synchronized` code. Part XI should cover both.
- **Part XX (20.5, 20.7):** deeper cache effects and false sharing (`perf c2c`), ordering costs on Arm (14.3 Systems
  exercise), `pause` latency and backoff tuning, the gateway per-worker counter design measured on Graviton.
- **Part XXII (22.7):** concurrency testing in CI: loom setup (`cfg(loom)`), TSan, Miri seeds, aarch64 stress runners.
- **Part XVII/XVIII:** the optimizations shown here (LICM hoisting, loop → `memcpy`, loop collapse) and their
  data-race-freedom license, from the compiler's side.

## Promises kept

- **PROGRESS "Part XIV: Relaxed vs Acquire/Release with the 'publish a buffer via counter' counterexample":** 14.3
  §3 (`ch03-01` Miri race, `ch03-02` fix, Miri weak outcomes in `ch03-03`).
- **1.3 → 14.3 (`Ordering::Relaxed` justified by join happens-before):** 14.2 §3 (rigorous proof).
- **1.1 → 14.5 (Java memory safety without data-race freedom, JLS 17.7):** 14.2 §7–§8, 14.5 §7.
- **3.3 → XIV (borrow rules ≈ MESI):** 14.1 §2 and §5 (where the analogy holds and where it stops).
- **5.2 / 9.5 → XIV (false sharing / `CachePadded` measured):** 14.4 §6 (`ch04-08`).
- **4.1 → XIV (atomics in the interior-mutability table):** 14.1–14.3.
- **Part I "memory ordering deserves more than a paragraph":** the whole Part.
- **Directive items:** SB litmus observed on real x86 hardware (14.1); x86 asm per ordering (14.3); CAS / ABA / epoch
  reclamation (14.4); Java `volatile` / `VarHandle` mapping (14.5).

## Meridian facts introduced

| System | Facts established | Where |
|---|---|---|
| Gateway drain handshake | per-worker `in_flight` flags + global `draining` flag = SB/Dekker pattern; first draft Release/Acquire rejected; all four accesses `SeqCst` with a documented comment; ~2 ns `xchg` per request | 14.1 §9 |
| Settlement-reconciliation worker (Rust batch) | `static mut STOP: bool` read by value, hoisted in release; pods ignored SIGTERM, SIGKILL after 30 s grace; a killed batch left a partial report file consumed downstream; fix `AtomicBool` `Relaxed`; rule: "a `static mut` in review is a question" | 14.1 §10 |
| Gateway process-wide metrics | `requests_total`, `bytes_out_total`, `panics_total` `AtomicU64` `Relaxed`, scraped every 15 s; no cross-counter consistency (documented); per-tenant billing counter pair uses `Mutex<(u64, u64)>` per shard | 14.2 §9 |
| Fraud library `Symbol` hash cache | ported Java `String.hashCode()`-style racy cache; caught by the Miri CI job added after Chapter 4.6; fix `AtomicU64` `Relaxed` (identical asm) | 14.2 §10 |
| Risk-limits snapshots | `RwLock` reader-count contention → immutable snapshots via `ArcSwap` | 14.3 §9 |
| Access-log shipper on Graviton | per-worker 256-slot batch, `len` `Relaxed` ("x86 is TSO"); correct for 8 months on x86; after a third of the gateway pool moved to Graviton: stale lines in 0.002% of batches, two cross-tenant exposures disclosed; fixes: Release/Acquire both directions, Miri test, aarch64 CI runners, review rule on `Relaxed` comments | 14.3 §10 |
| Gateway per-worker counters | Graviton profile: `requests_total.fetch_add` a top hot spot; first redesign `Vec<AtomicU64>` false-shared; shipped per-worker `CachePadded` blocks summed on scrape | 14.4 §9 |
| Risk-limits reservation slots | naive lock-free index free list → ABA double-booking at 3× load test; one-week investigation, reproduced only under oversubscription; shipped `Mutex<Vec<u32>>`, then per-worker slot ranges next quarter; rule: lock-free needs ABA + reclamation argument, Miri test, benchmark | 14.4 §10 |
| Fraud library concurrency port | translation sheet: `volatile Model` + DCL → `ArcSwap<Model>` (hourly reload); static-init map → `LazyLock`; `LongAdder` → per-worker `CachePadded`; `AtomicLong lastModelLoadMillis` → `AtomicU64` `Relaxed`; `ConcurrentHashMap` feature cache → sharded `Mutex<HashMap>`; `synchronized` → `Mutex` | 14.5 §9 |
| Model-reload flag incident | Java `volatile boolean reloadRequested` ported with `read_volatile`/`write_volatile`; Miri job (mandatory for `unsafe impl Sync` since the hash-cache incident) caught it; fix `AtomicBool` Release/Acquire; `clippy.toml` `disallowed-methods` bans `read_volatile`/`write_volatile` | 14.5 §10 |
| Market-data fan-out SPSC ring PR | Part XIV review capstone: feed-handler → fan-out thread ring replacing a crossbeam channel; x86 CI green, Miri red; 10 defects (both handovers `Relaxed`, `Sync` without `T: Send`, SPSC not enforced by types, no `Drop`, index overflow, `%` division, false sharing, "x86 is TSO" justification, capacity 0) | Part XIV review |
| Design exercises | latency histogram (64 buckets + count + sum, scraped every 15 s); settlement MPMC work queue (16 workers, p99 enqueue < 10 µs); 64-worker config snapshot; Meridian porting guide (concurrency section); multi-architecture test policy | 14.1–14.5 |

## Verification

- `listings/part-14/`: **38 files, 62 checks, all PASS** on rustc 1.98.1 (edition 2024): 25 `release ok`, 3 `debug ok`,
  2 `build` (debug) + 1 `release build`, 1 `test` (2 tests), 1 `panic`, 3 compile errors (E0499, E0432,
  `invalid_atomic_ordering`), 1 `debug+nightly error:deprecated`, and **25 Miri runs** (9 `miri` "Data race
  detected", 15 `miri-ok`, 1 `debug+tree miri-ok`). Full log: scratch `part-14/verify-full.txt` (61 checks) plus a
  separate run of the new `answers-ch01-yield-now.rs` (1 check).
- **Artifacts** (`tools/emit.ps1`, release): flag-loop asm (`ch01-03`), collapsed racy loop (`ch02-01`), identical
  racy/atomic hash-cache asm (`ch02-04`/`ch02-05`), orderings LLVM IR + asm (`ch03-04`), RMW and fence asm, `Arc` drop
  in `ch02-02`'s asm, and (new, for the answer key) `answers-ch01-yield-now.rs`: a `yield_now()` call keeps the
  `static mut` flag re-read each iteration (`call r14` / `cmp byte ptr [rbx], 1`).
- **Measurements** (one run each on the shared 4-vCPU Playground, labeled noisy): SB and MP litmus counts, ordering
  costs, CAS retries, seqlock reads/retries, false sharing, striped counter.
- A script confirmed all 5 `rust` / `rust,compile_fail` blocks in the chapters and review are verbatim substrings of
  verified listings. Other code blocks are `rust,ignore` excerpts naming their listing, or labeled sketches.
- **Unverifiable here, labeled in the text:** AArch64 asm (Playground is x86-64 only; local `--target` command given);
  HotSpot asm (needs a JDK + hsdis); jcstress; loom; TSan (nightly + build-std command given); `perf c2c`. Answer-key
  predictions (e.g., the unfenced-side SB exercise, the 8-thread false-sharing run) are marked "predicted from
  mechanism."

## Word count

README 622 · 14.1 4,261 · 14.2 4,251 · 14.3 4,211 · 14.4 4,839 · 14.5 3,601 · review 1,593 · answers 7,555 →
**30,933** (`wc -w`, code included).

## Tooling notes

- **Miri emulates weak memory.** Running a litmus test many times *inside one Miri run*, with the data itself atomic
  (so no race is reported), gives outcome counts: MP Relaxed 21/40, SB Relaxed 20/40, IRIW Release/Acquire 3/40
  (`ch03-03`, `ch03-05`). Miri's output was identical across re-runs (deterministic seed).
- Miri reports **retag races** ("retag write"/"retag read of type …") when a reference is created, not only on data
  accesses. The needle `Data race detected` covers both.
- **`crossbeam-epoch` 0.9.20 fails under Stacked Borrows** (`Local::element_of`) and passes under Tree Borrows: use
  `debug+tree miri-ok`. Give the structure its own `Collector` so dropping it runs deferred frees (otherwise Miri
  reports leaks of pending garbage).
- **Litmus tests on the Playground work** (AMD EPYC 9R14, `available_parallelism() = 4`): fresh atomics per round plus
  a two-thread spin barrier per round. Without the barrier, weak outcomes nearly vanish.
- `fetch_update` is deprecated on the Playground's nightly (1.100.0) in favor of `try_update`; stable 1.98.1 has
  `try_update` and `update`. Pattern: `debug ok` + `debug+nightly error:deprecated` in one listing.
- `std::sync::atomic::AtomicU128` doesn't exist for the baseline x86-64 target (E0432).
- In emitted asm, compiler-only fences appear as `#MEMBARRIER`. LLVM emits `lock or dword ptr [rsp - 64], 0` for
  `fence(SeqCst)`.
