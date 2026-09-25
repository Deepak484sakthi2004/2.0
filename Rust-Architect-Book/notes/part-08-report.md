# Part 08 report

## SUMMARY.md lines

Under `# Part VIII — Error Handling`:

```markdown
- [Part VIII Overview](part-08-error-handling/README.md)
  - [8.1 Result, Option, and the ? Operator](part-08-error-handling/ch01-result-option.md)
  - [8.2 Designing Error Types: Libraries vs Applications](part-08-error-handling/ch02-error-types.md)
  - [8.3 Panics, Unwinding, and Abort](part-08-error-handling/ch03-panics.md)
  - [8.4 Errors at Service Boundaries](part-08-error-handling/ch04-service-boundaries.md)
  - [Part VIII Review](part-08-error-handling/review.md)
```

Under the appendix answers list (after Part VII's line):

```markdown
  - [Part VIII Answers](appendix/answers-part-08.md)
```

## PROGRESS concepts

| Concept | Where | Full treatment planned |
|---|---|---|
| Option vs Result vs Result<Option<T>,E>; absence may default, malformed never does | 8.1 | — |
| `?` desugaring: Try::branch / FromResidual (unstable, [VERSION]); verified MIR of two `?`s | 8.1 | XVIII.4 (MIR) |
| E0277 for `?` in `()` fn, missing From, and Option-`?` in Result fn (all verified) | 8.1 | — |
| `main -> Result`: Termination prints `Error: {Debug}`, exit 1 (verified) | 8.1, 8.3 | — |
| `#[must_use]` Result warning text (verified); `-D warnings` / deny(unused_must_use) policy | 8.1 | XXII |
| Result sizes: Result<u64,()>=16, <&u64,()>=8, <NonZeroU32,()>=4, ParseIntError=1, io::Error=8, Result<(),io::Error>=8, Box<dyn Error+Send+Sync>=16, anyhow::Error=8 (verified) | 8.1 | V (niches) |
| Release asm of `?` happy path: Result<u32,ParseIntError> packed in rax, one test+branch per `?` | 8.1 | XX |
| Error type as API: programs (variants) vs people (Display) vs developers (Debug); chain rule (cause once, via source) | 8.2 | — |
| Library vs application errors; thiserror (expansion shown) vs anyhow ({}, {:#}, {:?}, downcast_ref, chain, root_cause) | 8.2 | XXII.1 |
| Box<dyn Error> / anyhow: 1 alloc per error, 0 on success (counting allocator, verified) | 8.2 | — |
| Large error cost: Result<u64,516-byte E>=520 B; sret + 504-byte memcpy per `?` layer vs cmove for boxed (verified asm); clippy result_large_err (unverified, labeled) | 8.2 | XX |
| std::backtrace::Backtrace: capture() Disabled by default; force_capture ~30 µs, first render ~4.6 ms (one run) | 8.2 | XIX |
| #[non_exhaustive] (no effect inside defining crate); opaque io::Error-style errors; don't leak dependency error types | 8.2 | XXII |
| Panic flow: hook → panic runtime → two-phase unwind → landing pads; payload types (&str vs String) | 8.3 | XVIII, XIX |
| Landing pad in release asm (`_Unwind_Resume` after `ret`) | 8.3 | XIX |
| Aborts: double panic ("panic in a destructor during cleanup"), extern "C" since 1.81 ("panic in a function that cannot unwind"), panic=abort (process::abort analog) — all verified | 8.3 | XVI |
| Exit statuses verified by re-exec: Err→1, panic→101, exit(3)→3, abort/extern-C→signal 6 | 8.3 | XIX.4 |
| Panic cost measured: Err 3.0/15.8 ns vs panic+catch_unwind 1.8/3.2 µs at depth 1/10 (release, one run) | 8.3 | XX |
| Mutex poisoning (into_inner, clear_poison 1.77), join() Err payload, UnwindSafe/AssertUnwindSafe | 8.3 | XI.3 |
| Panic boundaries: hook + per-request catch_unwind; Tokio JoinError::is_panic (verified); FFI entry catch_unwind → codes; extern "C-unwind" | 8.3 | XIII.2, XVI.3 |
| Fallible cleanup: Tx::commit(self) -> Result<Committed, CommitError{RolledBack, OutcomeUnknown}> | 8.3 | XXIII.6 |
| Error taxonomy: Rejected / Transient / Ambiguous / Internal; four boundary questions | 8.4 | XXI.4, XXIV.1 |
| Exhaustive error→status mapping (E0004 on new variant, verified); stable codes; safe client bodies with request_id | 8.4 | XXI.6, XXII.3 |
| Retries: exponential backoff + full jitter, deadline budget, Retry-After, retry only transient / ambiguous-if-idempotent (verified sim) | 8.4 | XXI.4 |
| Idempotency keys: store with effect, fingerprint, in-progress + conflict; key-per-attempt bug (verified) | 8.4 | XXIV |
| Timeouts ambiguous at the socket; io::ErrorKind by phase (refused vs reset vs read timeout) | 8.4 | XXI.1 |
| Retry amplification (27×), retry budgets, deadline propagation, fail open/closed | 8.4 | XXI.4 |
| Observability: log once at boundary, level by class, chain in logs only, metric labels low-cardinality (tracing output verified) | 8.4 | XXII.5 |

## Promises to later Parts

- **Part XI:** `lock().unwrap()` poisoning policy per lock (8.3 §5, §10); worker pools that survive job panics (8.3
  exercise); Ferrite v1 panic policy for poisoned shard locks and thread-pool capacity (8.3 design exercise).
- **Part XIII:** don't hold locks across `.await`; `Box<dyn Error>` without `Send + Sync` can't be held across `.await`
  in a multi-threaded runtime (8.2 §4); Ferrite v2 `-ERR <CODE>` error protocol, busy/shutting-down codes (8.4 design).
- **Part XVI:** full FFM binding for `meridian_score` (error codes 0 / -1 / -99, catch_unwind at every entry, 8.3 §7);
  `extern "C-unwind"`.
- **Part XIX:** unwind tables (.eh_frame, .gcc_except_table / LSDA) in the binary; symbolization of backtraces.
- **Part XX:** measure panic cost vs depth (8.3 exercise), Result-vs-exception failure-rate benchmark (8.1 exercise).
- **Part XXI (21.4):** timeouts, retries, circuit breakers as network mechanisms; retry budgets; deadline propagation.
- **Part XXII:** `thiserror`/`anyhow` in the crate-choice chapter (22.1); axum `IntoResponse for PaymentError` (22.3,
  unverified sketch in 8.4 §8); tower Retry policy using `class()`; `tracing` observability (22.5); clippy
  `result_large_err` (8.2 §5, unverified).
- **Part XXIII:** Ferrite v3 `FerriteError` and the KvStore trait becoming fallible (8.2 design exercise); commit with
  unknown outcome (8.3 §7).
- **Part XXIV:** idempotency + reconciliation for ambiguous outcomes; "did it happen?" as the core distributed failure
  question (8.4).

## Promises kept

- Part VIII (PROGRESS): replace `String` errors in the Chapter 2.4/2.5 header parser → `HeaderError` enum with
  before/after and a verified behavior flip on message rewording (8.2 §1, §3, §10).
- Replace `String` errors in Project L1 `logstat` → `ArgsError` + `LogstatError` with `exit_code()`/`is_benign()`,
  before/after, tests (8.2 §9, listing `ch02-07-logstat-errors.rs`).
- `?` explained properly (L1/L2 used it): desugaring + verified MIR + release asm (8.1 §3–§6).
- Panic strategy per system from Chapter 2.2 (Tokio unwind, FFM lib unwind + catch_unwind, CLI abort): all three
  verified (8.3 §7).
- Unwind cleanup MIR from Chapter 3.5: referenced, and shown as a real landing pad in release asm (8.3 §4).
- Drop can't fail (3.5) → explicit fallible close; transaction guard `commit` returning `Result` (the 3.5 Advanced
  exercise, consistent with answers-part-03's "outcome unknown: reconcile") (8.3 §7, listing `ch03-10`).
- Chapter 1.3/Part I "Part VIII covers the trade-off" (unwind vs abort): 8.3 §4, §7.
- Chapter 2.2's `debug_assert!` discount incident referenced in the assert policy (8.3 §7).

## Meridian facts introduced

| System | Facts established | Where |
|---|---|---|
| Gateway config loader (Rust) | ~40 settings; 2025 Java incident: `UPSTREAM_TIMEOUT_MS=250ms` swallowed by catch → kept 1,000 ms default for 40 min; rule "absence may default, malformed never" | 8.1 §9 |
| Settlement batch job (Rust) | ignored `remove_file` Result + 300 warnings → stale lock, settlement a day late; CI now `-D warnings`, `deny(unused_must_use)` | 8.1 §10 |
| Market-data ingest (Rust, fan-out rewrite) | String-error dispatch `e.contains("need")`; reworded message → split frames closed connections → reconnect storm, stale prices ~25 min; `HeaderError::Incomplete` fix; review rules (no String errors in libs, no matching on messages) | 8.2 §10 |
| Ledger library | `LedgerError` (thiserror): AccountNotFound, InsufficientFunds, Storage(#[from] io), Corrupt{line, source} | 8.2 §3 |
| Gateway panic boundary | hook → one structured line `code=INTERNAL_PANIC` + `panics_total`; per-request/task containment → 500 INTERNAL; alert on panics_total > 0 | 8.3 §9 |
| Gateway double-panic incident | handler panicked holding pool mutex → poisoned; pooled-conn Drop `lock().unwrap()` panicked during unwinding → abort, ~150 in-flight requests/pod lost (Little's law arithmetic), exit 134, rolling restarts | 8.3 §10 |
| Fraud FFM library | `meridian_score` entry: catch_unwind → rc 0 / -1 (invalid) / -99 (panic); Java maps -99 to IllegalStateException + alert | 8.3 §7 |
| payments-core (new Rust service) | orchestrates charges gateway → payments-core → card processor (Java ledger behind it); `PaymentError` taxonomy with codes PAY_INVALID_AMOUNT/…/PAY_INTERNAL, classes Rejected/Transient/Ambiguous/Internal, statuses 400/402/409/429/503/504/500 | 8.4 §1, §3 |
| Double-charge incident (Java, March 2025) | gateway generic client retried POST /charges after 2 s payments-upstream read timeout; 1,140 customers double-charged over 40 min | 8.4 §9 |
| payments-core idempotency | mandatory Idempotency keys on POST /charges, one UUID per checkout attempt, stored with the charge (unique (tenant,key)), derived key forwarded to processor; one retry layer (gateway); reconciliation job for PAY_PROCESSOR_TIMEOUT | 8.4 §9 |
| 4xx page storm (Java payments) | declines logged at ERROR; Black Friday 40K ERROR lines/min; muted alert buried a ledger fsync EIO; fix: level by class, `payment_errors_total{code,class}`, alert on internal class + SLO burn | 8.4 §10 |
| Refunds PR | Part VIII review artifact: 14 defects (unwrap on input, negative amounts, stringly retry, retry w/o key, fall-through 200 with empty id, leaked host IP, processor-before-ledger ordering, Box<dyn Error>, panic on business rule, log-and-return, Debug to client, swallowed audit, path traversal + truncating audit file) | Part VIII review |

## Verification

- `listings/part-08/`: **38 files, 42 checks, all pass** on the Playground (rustc 1.98.1, edition 2024):
  24 `debug ok`, 2 `release ok` (error costs, panic cost), 4 `build`, 5 `crash` (main Err "Error:", anyhow main
  "Caused by", extern "C" "cannot unwind", double panic, process::abort with an `eprintln!` marker), 1 `test`
  (2 tests), 3 `error:E0277`, 1 `error:E0004`, 2 `miri-ok` (FFI boundary with `out.write`, counting allocator + anyhow). Final run saved to scratch `part-08/verify-final.txt`.
- Crates exercised: thiserror 2.x, anyhow, tempfile, serde_json, tracing + tracing_subscriber, tokio.
- Artifacts via `tools/emit.ps1`: MIR (debug) of `sum_two` (Try::branch / FromResidual); release asm of
  `sum_two`/`parse_u32`, `outer_big`/`outer_boxed`/`check_big`/`check_boxed`, `with_guard` landing pad; nightly macro
  expansion of the thiserror enum. All quoted trimmed; call syntax simplified and noted.
- Measurements (one run, noisy, labeled): backtrace capture/render times; panic vs Err cost (release).
- `unsafe` appears only in the FFI listing (`out.write(v)`, SAFETY comment) and the counting allocator (Part III
  pattern, SAFETY comment); both listings pass Miri (`miri-ok`).
- Unverifiable items, labeled in text: clippy `result_large_err` threshold ("not verified here: requires cargo clippy";
  128 bytes "at the time of writing"); axum `IntoResponse` mapping (text only, "not on the Playground"); Java/Go snippets
  are illustrative; HTTP/gRPC/RFC facts and the Brooker 2015 jitter article attributed; Meridian incidents fictional.
- Observed facts worth knowing: panic output format is `thread 'main' (14) panicked at src/main.rs:L:C:`; a nested
  panic (extern "C" abort, double panic) prints a full backtrace without RUST_BACKTRACE; `process::abort` produces
  empty stderr on the Playground (hence the marker line for the `crash` needle).

## Word count

Chapters (wc -w, including code and output): 8.1 5,146 · 8.2 5,235 · 8.3 5,056 · 8.4 4,619 · README 733 ·
review 1,530 · answers 4,706 → **27,025** total.

## Tooling notes

- `crash` checks need a needle in stderr; `std::process::abort()` writes nothing to stderr on the Playground, so print
  a marker with `eprintln!` before aborting.
- Re-executing the current binary with `std::process::Command::new(std::env::current_exe()?)` works on the Playground:
  handy for verifying exit codes and signals (`ExitStatusExt::signal()`).
- `tools/emit.ps1 -Target expand` works with Playground crates (thiserror expansion succeeded on nightly).
- `tracing_subscriber::fmt().without_time().with_target(false).with_ansi(false).with_writer(io::stdout)` gives
  reproducible, colorless log output on stdout.
- Rvalue static promotion doesn't cover `Duration::from_millis(..)` inside `&[...]` literals (E0716): build test tables
  as `Vec`s or `const` items.
- Edition 2024 denies references to `static mut` (e.g., in `format!`); use a closure counter or an atomic.
- `PanicHookInfo::payload_as_str()` is available on 1.98.1.
