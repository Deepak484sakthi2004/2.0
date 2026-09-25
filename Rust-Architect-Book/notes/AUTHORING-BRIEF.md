# Authoring brief for Part writers (Parts V–XXVI)

You are writing one Part of **Source to Silicon: Rust for the Systems Architect** at `C:\Users\Deepak\Rust-Architect-Book`.
Parts I–IV are complete and are the quality bar. Match their depth, voice, structure, and verification standard exactly.
Re-read these exemplars on disk before writing:

- `src/part-03-ownership/ch03-borrowing.md` (chapter template, verified assembly, Java comparison, Meridian scenario)
- `src/part-04-borrow-checker/ch01-aliasing-xor-mutation.md` (verified IR, tables, failure scenario)
- `src/part-03-ownership/project-02-redact.md` + `listings/part-03/project-02-redact.rs` (project chapter + tested listing)
- `src/part-04-borrow-checker/review.md` and `src/appendix/answers-part-04.md` (review + answer key format)
- `SPEC.md` (the reader's brief: your Part's section + the global rules), `CLAUDE.md`, `PROGRESS.md`

## Non-negotiables

1. **Chapter template** (every chapter): header block (where this sits / prerequisites / after this chapter you can);
   `Pass 1 · User level` (1 Problem, 2 Mental model, 3 Rust code) → `Pass 2 · Systems level` (4 Under the hood,
   5 Memory, 6 CPU / OS) → `Pass 3 · Architect level` (7 Trade-offs, 8 Java comparison, 9 Production scenario,
   10 Failure scenario) → `Practice` (11 Interview & architecture questions, 12 Exercises: beginner / intermediate /
   advanced / systems / architecture, 13 Debugging exercise, 14 Design exercise). Target ~3,500–5,000 words per chapter.
2. **Layer tags** on strong claims: [LANG] [RUSTC] [RUNTIME] [OS] [CPU] [LIB] [VERSION]. Never present implementation
   details as guarantees.
3. **Every Rust code block that is claimed to compile/run is backed by a listing file** in `listings/part-NN/` with
   `// verify:` headers, and was verified with `tools/verify.ps1` (outcomes: ok, build, test, panic, crash,
   error:E0xxx, error:<word>, miri, miri-ok; edition override `debug@2021`; Tree Borrows instead of Stacked Borrows
   for Miri: `debug+tree miri-ok`). Quote **real** compiler output and program
   output (trimmed only; say "labels simplified" if you edit asm labels). `rust,compile_fail` blocks show the real error.
   Use `rust,ignore` for excerpts of a verified listing (say which listing) or for clearly-labeled unverified sketches.
4. **Compiler artifacts** (MIR/LLVM IR/asm/macro expansion) come from `tools/emit.ps1`; use `#[inline(never)]` on
   functions you inspect in a lib crate. At least one real artifact or measurement per Part where it is meaningful.
5. **Numbers**: measured (say how/where; timings on the Playground are "one run, noisy"), order-of-magnitude (labeled),
   or attributed (who, year). No invented benchmark results. Performance rankings without measurement are labeled
   "predicted from mechanism" and paired with an exercise to measure.
6. **Honesty about limits**: things that can't be verified on the Playground (C code, multi-crate workspaces, internet
   networking, `perf`, crates not on the Playground) are shown as `rust,ignore` / other-language blocks with an explicit
   "not verified here: requires …" note and the exact command to verify locally. Keep these minimal.
7. **Java comparison** in every chapter where useful, with an **Analogy limit** box. **Meridian** (fictional
   payments-and-marketplace company; facts in `PROGRESS.md`) for production scenarios; keep facts consistent.
8. **No answers inline.** Answers go to `src/appendix/answers-part-NN.md` (same structure as answers-part-04.md: per
   chapter: interview questions, debugging exercise, selected exercises; then the review capstone; then interview mode).
9. Counting allocator (Part III pattern, with its SAFETY comment) when claims are about allocations; Miri
   (`// verify: debug miri <needle>` / `miri-ok`) for any `unsafe` example.

## Files you own (and ONLY these)

- `listings/part-NN/*.rs` (NN = two-digit Part number)
- `src/part-NN-<slug>/README.md` (Part overview, like `src/part-04-borrow-checker/README.md`), chapter files
  `chMM-<slug>.md`, project files `project-LL-<name>.md` (LL = project level, two digits), `review.md`
- `src/appendix/answers-part-NN.md`
- `notes/part-NN-report.md` (your report, format below)

**Do NOT edit**: `src/SUMMARY.md`, `PROGRESS.md`, `CLAUDE.md`, `README.md`, `SPEC.md`, `book.toml`, `tools/*`, other
Parts, other notes. **Do NOT run git** or push. **Do NOT touch** the memory directory. Other Parts are being written in
parallel: reference them by chapter number and title from `src/SUMMARY.md` (and by content only if the file already
exists on disk). Use your own scratch folder: `<scratchpad>/part-NN/`.

## Playground facts

- rustc 1.98.1 stable, edition 2024 (nightly for Miri and `-Target hir/expand`). Linux x86-64.
- Crates available (use directly, no Cargo.toml): tokio (+ tokio_util, tokio_stream), futures, hyper (+ hyper_util,
  http, http_body_util, httparse), tower (+ tower_http), serde, serde_json, rayon, crossbeam (+ channel/epoch/deque),
  parking_lot, bytes, anyhow, thiserror, tracing (+ tracing_subscriber), clap, memchr, libc, rand, sha2, ring, regex,
  once_cell, smallvec, ahash, fxhash, foldhash, hashbrown, indexmap, slab, arc_swap, petgraph, hdrhistogram, rusqlite,
  memmap, tempfile, nom, rkyv, bytemuck, zerocopy, bumpalo, crossbeam_epoch, rustls, tokio_rustls, socket2, mio,
  flate2, zstd, csv, chrono, uuid, itertools, num_cpus, thread_local, sharded_slab, url, base64, toml, serde_yaml.
- NOT available: axum, sqlx, diesel, criterion, loom, proptest, dashmap, slotmap, cranelift, inkwell, wasmtime.
- No internet. Localhost sockets inside one program may work (test before relying on it). Probably a writable `/tmp`.

## Ferrite (the reader's system across projects) — shared contract

- **v1 (Part XI, Project L4)**: library + threaded TCP server (std::net + the thread pool from Project L3).
  `pub trait KvStore { fn get(&self, key: &[u8]) -> Option<Vec<u8>>; fn put(&self, key: Vec<u8>, value: Vec<u8>) ->
  Option<Vec<u8>>; fn delete(&self, key: &[u8]) -> Option<Vec<u8>>; }` implemented by a sharded
  `ShardedStore` (`Vec<RwLock<HashMap<Vec<u8>, Vec<u8>>>>`, shard = hash(key) % N). Line protocol (UTF-8 tokens without
  whitespace in v1): requests `PING`, `GET <key>`, `SET <key> <value>`, `DEL <key>`; responses `+PONG`, `+OK`,
  `$<value>`, `_` (nil), `-ERR <message>`; each terminated by `\n`.
- **v2 (Part XIII, Project L5)**: same protocol over Tokio: connection limit, per-connection backpressure, timeouts,
  graceful shutdown; reuses `ShardedStore` behind `Arc`.
- **v3 (Part XXIII, Project L7)**: persistent engine: WAL + memtable + sorted immutable segment files (LSM),
  recovery on startup, explicit fsync policy; implements `KvStore`.
- **v4 (Part XXIV, Project L10)**: mini database: range scans, a small query/command layer, snapshot (MVCC) reads.
- **v5 (Part XXIV, Project L11)**: replication: leader/follower log shipping, simplified Raft-style election, key-range
  partitioning, failure tests with an in-process simulated network.

## Meridian consistency across parallel writers

Before inventing Meridian facts, read the Meridian tables in `PROGRESS.md` **and** in every existing
`notes/part-*-report.md` (writers of other Parts add facts there before the integrator merges them). Reuse established
numbers verbatim (gateway 400K req/s, ~330 cores, 55 pods; fraud 50K scores/s; ledger ~3K TPS; payments-core taxonomy;
…). New incidents should be scoped to a new subsystem or a clearly dated event, so they can't contradict another Part.

## Tooling gotchas learned in Parts V–IX (read before writing listings)

- `error:<word>` needles are one `\w+` word: use `error:lifetime` for "lifetime may not live long enough" (no code).
- `crash` needs a needle in stderr; `std::process::abort()` prints nothing on the Playground, so `eprintln!` a marker
  first. Re-exec via `Command::new(std::env::current_exe()?)` works (exit codes, signals via `ExitStatusExt`).
- Panic output format on 1.98.1: `thread 'main' (14) panicked at src/main.rs:L:C:`; unnamed-thread overflow prints
  `thread '<unknown>' (NN) has overflowed its stack`.
- Edition 2021+ disjoint closure capture: `move || s.field` captures only the field; force a whole-value capture when
  demonstrating auto-trait (Send/Sync) errors.
- Edition 2024 denies references to `static mut` (even in `format!`): use atomics or a closure counter.
- `#[derive(Debug)]` on `Foo<S>` with uninhabited marker types fails only when used (E0277): implement manually.
- LLVM merges identical functions in release asm (`a = b` alias lines); a tiny `#[inline(never)]` callee can still
  vanish from its caller via `returned`-argument propagation. Check callee and caller.
- v0 symbol mangling is the default: IR `define` lines are `_R…`, preceded by a demangled `; crate::path::<Args>` comment.
- Debug IR of generic-heavy code can be ~20 MB: extract numbers, then delete it (disk is tight).
- `tools/emit.ps1 -CrateType bin` pulls functions out of binary listings; `-Target expand` works with Playground crates.
- Bash paths passed to `verify.ps1` must use forward slashes (`"C:/…/$f"`).
- `tracing_subscriber::fmt().without_time().with_target(false).with_ansi(false).with_writer(io::stdout)` gives
  reproducible log output. A counting `BuildHasher` (thread_local counter in `finish()`) counts hashes.
- Stack-overflow tests: measure the frame size in-process, then run at 90% (ok) and 110% (crash) of the limit on an
  explicitly sized `thread::Builder` thread.

## Report file: `notes/part-NN-report.md`

```text
# Part NN report
## SUMMARY.md lines          (exact markdown list lines for your Part, with relative links, to paste under the Part heading)
## PROGRESS concepts          (table rows: | Concept | Where | Full treatment planned |)
## Promises to later Parts    (bullets)
## Promises kept              (which earlier promises you fulfilled)
## Meridian facts introduced  (table rows: | System | Facts established | Where |)
## Verification               (files, checks, all pass? Miri runs? anything unverifiable and how it is labeled)
## Word count                 (chapters + review + answers)
## Tooling notes              (anything future writers must know)
```

Your final message: 3–6 lines (report path, counts, notable issues). The integrator reads the report file.

## Part map (slugs and chapters are fixed by src/SUMMARY.md)

| Part | Folder | Chapters / projects |
|---|---|---|
| V | part-05-types-architecture | 5.1–5.4 |
| VI | part-06-traits | 6.1–6.5 |
| VII | part-07-generics | 7.1–7.3 |
| VIII | part-08-error-handling | 8.1–8.4 |
| IX | part-09-collections | 9.1–9.5 + Interlude (BFS vs DFS) |
| X | part-10-iterators | 10.1–10.4 |
| XI | part-11-concurrency | 11.1–11.7 + Project L3 (HTTP server) + L4 (Ferrite v1) |
| XII | part-12-async | 12.1–12.6 |
| XIII | part-13-tokio | 13.1–13.5 + Project L5 (Ferrite v2) |
| XIV | part-14-memory-model | 14.1–14.5 |
| XV | part-15-unsafe | 15.1–15.6 |
| XVI | part-16-ffi | 16.1–16.4 |
| XVII | part-17-compilers | 17.1–17.8 |
| XVIII | part-18-rustc | 18.1–18.7 |
| XIX | part-19-binary-os | 19.1–19.6 |
| XX | part-20-performance | 20.1–20.7 |
| XXI | part-21-networking | 21.1–21.6 |
| XXII | part-22-ecosystem | 22.1–22.7 + Project L6 |
| XXIII | part-23-storage | 23.1–23.6 + Project L7 (Ferrite v3) |
| XXIV | part-24-distributed | 24.1–24.5 + Projects L8, L9, L10 (Ferrite v4), L11 (Ferrite v5) |
| XXV | part-25-blockchain | 25.1–25.4 |
| XXVI | part-26-language | 26.1–26.7 + Projects L12, L13 + Final Capstone |
