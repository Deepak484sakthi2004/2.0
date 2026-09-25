# Appendix A — Answer Key: Part VIII

> Model answers. Write yours first. Where several answers are defensible, the key says so.

---

## Chapter 8.1 — Result, Option, and the `?` Operator

### Interview & architecture questions

**1. Choosing the type.** `Option<T>` when "nothing" is a normal answer the caller expects (`map.get`, `iter.next`).
`Result<T, E>` when something went wrong and the caller may need to know what (`parse`, `File::open`).
`Result<Option<T>, E>` for a lookup that can both *fail* and legitimately *find nothing*: `find_user(id)` returns
`Ok(None)` for "no such user" and `Err(_)` for "the database is unreachable". It prevents the classic bug of treating an
outage as absence: a Java `Optional<User> findUser()` that returns `Optional.empty()` on `SQLException` lets the caller
create a duplicate account, or grant default permissions, while the database is down.

**2. Desugaring `?`.** Conceptually:
`let x = match Try::branch(f()) { ControlFlow::Continue(v) => v, ControlFlow::Break(r) => return FromResidual::from_residual(r) };`.
For `Result` that reduces to `match f() { Ok(v) => v, Err(e) => return Err(From::from(e)) }`. The conversion happens
inside std's `impl<T, E, F: From<E>> FromResidual<Result<Infallible, E>> for Result<T, F>`, so the bound that must hold is
`MyError: From<E>`. `Try` and `FromResidual` are unstable; the behavior for `Result` and `Option` is stable.

**3. `?` on `Option` in a `Result` function.** It would need `impl FromResidual<Option<Infallible>> for Result<T, E>`.
std deliberately doesn't provide one: a `None` carries no error value, so the conversion would have to invent one. Fixes:
`.ok_or(MyError::Missing("key"))?` (or `ok_or_else` if building the error is costly), `let Some(v) = ... else { return
Err(...) };`, or change the function to return `Option` if absence is all it can report.

**4. Why both are E0277.** E0277 means "a required trait bound isn't satisfied". Using `?` in a `()`-returning function
fails the bound `(): FromResidual<...>`. Using `?` without a conversion fails `ConfigError: From<ParseIntError>`. `?` is
defined through traits, so its errors are trait errors.

**5. `Result<(), io::Error>` is 8 bytes.** [LIB] On 64-bit targets std packs `io::Error` into one tagged pointer (OS code,
simple kind, static message, or boxed custom error, distinguished by low bits), and that value is never zero, so rustc
uses zero to encode `Ok(())`. Guarantees vs details: the null-pointer niche is *guaranteed* for `Option` of references,
`Box`, `NonNull`, and similar types, and std also documents FFI-compatible layouts for some `Result`s with a zero-sized,
align-1 error side; check the documentation of your version. `io::Error`'s representation and the resulting 8 bytes are
**implementation details** [RUSTC] [LIB] that could change.

**6. Cost of `?` vs exceptions.** Release builds turn each `?` into a `test` and a conditional jump that's almost never
taken (verified in `sum_two`): about a cycle when predicted. The error path is a few instructions. A table-based exception
costs nothing on the happy path and microseconds when thrown (unwind-table lookups per frame, a personality routine,
payload allocation; Java adds stack-trace capture). It matters when failure is *common*: parsing untrusted input,
validation layers, probing lookups. There, `Result` has a flat, predictable cost, while exceptions make the failure path
tens to hundreds of times slower and push APIs into duplicate "try" variants.

**7. `main` returning `Err`.** [LIB] It prints `Error: {e:?}` (**Debug**) to stderr and exits with status 1. Debug output is
for developers: struct dumps like `ParseIntError { kind: InvalidDigit }`, no cause chain, no guarantee of stability, and
one exit code for every failure. A user-facing CLI should print `Display` plus the chain and choose exit codes itself
(`logstat`'s `exit_code()`), or use `anyhow`, whose `Debug` is written as a human-readable report.

**8. `unwrap_or_default()`.** Correct when the default is the right *meaning of absence*: an optional display name
becomes empty, an optional tag list becomes empty. Wrong when it hides a *failure*: a parse error becoming 0, a malformed
timeout becoming the default (the §9 incident), a failed lookup becoming "no permissions". Ask "if this value were
malformed, would I want the process to carry on silently?"

### Debugging exercise (`pool_size`)

1. The function would need `impl FromResidual<Option<Infallible>> for Result<u32, ParseIntError>`. std doesn't provide it
   because `None` has no error to convert, and any automatic conversion would have to make one up.
2. You'd have to pass a `ParseIntError`, and you can't construct one directly: its fields are private. The only way is a
   deliberately failing parse (`"".parse::<u32>().unwrap_err()`), which produces "cannot parse integer from empty
   string" for a *missing* variable, a lie in the error message. The dead end tells you the error type is wrong: the
   function can fail in two different ways (missing, malformed), so it needs its own enum.
3. Required: `enum PoolSizeError { Missing, Invalid(ParseIntError) }` with `From<ParseIntError>`, and
   `env.get("POOL_SIZE").ok_or(PoolSizeError::Missing)?.parse()?`. Defaulted: `match env.get("POOL_SIZE") { None =>
   Ok(16), Some(raw) => raw.parse() }` (or `env.get(..).map(|r| r.parse()).transpose().map(|o| o.unwrap_or(16))`).
   §9's rule: absence may default, malformed never does. It depends on the setting: a pool size has a safe default, a
   database URL doesn't.

### Selected exercises

- **Intermediate:** `lines.map(parse_kv).collect::<Result<HashMap<_, _>, _>>()` stops at the first error. To report all
  of them, collect every result and `partition(Result::is_ok)`, or push errors into a `Vec` while building the map.
  People editing a config file prefer all errors: one edit-and-retry cycle instead of one per mistake.
- **Advanced:** `raw.map(|s| s.parse::<u32>()).transpose().map_err(ConfigError::Invalid)` is compact. The `match`
  (`None => Ok(None), Some(s) => s.parse().map(Some).map_err(ConfigError::Invalid)`) is clearer to a reviewer new to
  Rust. Either is fine; be consistent.
- **Systems:** with a 64-byte error, the `Result` no longer fits in two registers, so it's returned through a hidden
  pointer to caller-reserved memory (as in Chapter 8.2's `outer_big`). The timing comparison should show the `Result`
  version at nanoseconds per call and the panic version at microseconds per failing call. It's noisy because the
  Playground shares machines and the run is short.

---

## Chapter 8.2 — Designing Error Types: Libraries vs Applications

### Interview & architecture questions

**1. Two audiences.** Programs need to decide (retry, wait, which status, which exit code): the **variants** and their
fields serve them. People need to understand: each layer's **`Display`**, joined through **`source()`**, serves them.
Developers also get `Debug` and, when enabled, a backtrace.

**2. The chain rule.** A layer's `Display` describes what *that layer* was doing and doesn't include its cause; the cause
is returned by `source()`, and a reporter walks the chain. Violate it one way (cause in `Display` *and* in `source()`),
and every chained report prints the cause twice. Violate it the other way (cause only in `Display`, `source()` returns
`None`), and programs can no longer inspect the cause: that's what `logstat`'s `io::Error::new(kind, format!(...))` did.

**3. `thiserror` vs `anyhow`.** Use an enum (hand-written or `thiserror`) when some code will **branch** on which error
it is: libraries, domain modules whose errors map to statuses, CLIs whose errors map to exit codes. Use `anyhow` when
errors are only **reported**: application glue, startup, jobs, `main`. It isn't strictly library vs binary: `logstat` is a
binary and uses an enum because `main` branches on it; a library may use `anyhow` internally but shouldn't expose it.

**4. What `#[from]` generates.** A `From<Inner>` impl that constructs the variant, and a `source()` arm returning the field.
`#[error("...")]` generates the `Display` arm. The expansion is ordinary impls of std traits (verified), so `thiserror`
doesn't appear in your public API, and replacing it with hand-written impls isn't a breaking change.

**5. 8 vs 16 bytes.** `Box<dyn Error + Send + Sync>` is a fat pointer: data pointer plus vtable pointer. `anyhow` stores
the vtable pointer *inside* the heap allocation, next to the error, so its handle is one thin pointer. Converting a
concrete error into either costs one allocation (and one free when dropped): verified 1/1 on failure, 0/0 on success.
Each `.context()` layer adds an allocation.

**6. Large errors on the success path.** The caller must reserve space for the whole `Result`. At 520 bytes it can't be
returned in registers, so it's written through a hidden pointer (sret) to a caller-reserved stack slot: even the `Ok`
path loads and stores through memory, and each layer reserves its own buffer (528-byte frames in the verified assembly).
On `Err`, every `?` copies the error into the next caller's slot (a 504-byte `memcpy` per layer). Fixes: box the error or
its large variant, or move bulky context out of the error.

**7. `#[non_exhaustive]`.** Inside the defining crate: no effect, so your own `match`es stay exhaustive and adding a
variant breaks your mapping code (which you want). Outside: downstream matches must have a wildcard arm, so adding a
variant isn't a breaking change for them. You keep compile-time completeness where you control the code, and your callers
keep compatibility.

**8. Backtraces.** Capturing walks the stack (tens of microseconds, measured once), and symbolization reads debug info
(milliseconds on first display). Most errors are expected outcomes whose stack is irrelevant, so the default is off,
enabled by environment variable when debugging. Force a capture where an error means a **bug** (an invariant violation,
an Internal-class error), ideally rate-limited, and never for Rejected-class errors like a declined card.

### Debugging exercise (doubled cause)

1. Two mechanisms print `ArgsError`: `#[error("{0}")]` puts it in `LogstatError`'s `Display`, and `#[from]` implies
   `#[source]`, so `source()` returns it as well. `render` prints the `Display`, then walks `source()`, so it appears
   twice.
2. `#[error(transparent)] Usage(#[from] ArgsError)`: `Display` *and* `source()` forward to the inner error (whose source
   is `None`), printing `logstat: --top needs a value`. Or keep the source and change the message:
   `#[error("invalid arguments")]`, printing `logstat: invalid arguments: --top needs a value`.
3. `thiserror` treats a field **named `source`** as the error's source even without the attribute.
4. `ErrorKind::NotFound.into()` builds the "simple" representation, which has only a kind and displays the kind's
   description, "entity not found". An error from a failed system call carries the OS error code and displays the OS
   message plus "(os error 2)". Prefer the latter in logs: the errno is precise. (Tests that build errors from kinds
   should expect the kind's text.)

### Selected exercises

- **Advanced (the `io::Error` pattern):** with the representation private, you can later change boxing, add context
  fields, merge or split internal cases, change which details are captured, and add new `ErrorKind` variants (the kind
  enum is `#[non_exhaustive]`), all without a semver-major release. With a public enum, adding a field to a variant or
  changing a payload type breaks callers.
- **Systems:** predicted from mechanism: the first `.context()` on a `Result<_, std error>` allocates once (it boxes the
  context and the original error together), and each further `.context()` on an `anyhow::Result` allocates once more; so
  two layers means two allocations on failure and none on success. Measure it. On x86-64 with rustc 1.98, a return value
  larger than 16 bytes (two registers) is returned through a hidden pointer [RUSTC]; the Rust ABI is unspecified, so this
  threshold is observed, not guaranteed.

---

## Chapter 8.3 — Panics, Unwinding, and Abort

### Interview & architecture questions

**1. `Result` or panic?** Results: invalid input, a missing file, a timeout. Panics: an index computed by our own code is
out of range, a ledger's debits don't equal its credits after a commit, a state machine reaches a state its transitions
make impossible. The test: does the failure come from outside the program (input, environment, dependencies), and can a
caller do something useful about it? Then it's a `Result`. If it means our code is wrong, it's a panic.

**2. From `panic!` to `catch_unwind`.** `panic!` → `core::panicking::panic_fmt` → std's panic handler → panic count
incremented → **hook runs** (default: prints the message) → the panic runtime starts unwinding → search phase: walk the
frames via unwind tables to find `catch_unwind`'s frame → cleanup phase: jump into each frame's landing pad, which drops
its locals and resumes → `catch_unwind` returns `Err(payload)`. The hook runs *before* unwinding, so the stack is intact
for a backtrace, and it runs even under `panic = "abort"`, so the message is never lost.

**3. Landing pads.** Cleanup code for a call site, reachable only by the unwinder, which finds it through the LSDA table.
In `with_guard`, the normal path is `call may_panic`, `call drop`, `ret`; the pad sits after the `ret`: save the
exception object, drop the guard, `call _Unwind_Resume`. "Zero-cost": the normal path executes no unwinding instructions.
The remaining costs: values that need dropping are kept addressable in memory (the guard's stack slot), code size grows,
and the possible unwind edges constrain some optimizations.

**4. Aborts even with `unwind`.** A panic while already panicking (a destructor panics during unwinding); a panic
reaching an `extern "C"` boundary (since 1.81); allocation failure (the default `handle_alloc_error`); stack overflow
(not a panic, but an abort). Plus, of course, `panic = "abort"` itself and `process::abort()`.

**5. What `catch_unwind` doesn't catch.** Aborts of every kind, and anything at all under `panic = "abort"`. It isn't
`try`/`catch` because the payload is untyped (`Box<dyn Any + Send>`), it can't select by type, it's slow (microseconds),
the application's profile may disable it, and it exists to build boundaries, not control flow.

**6. Poisoning.** When a `MutexGuard` is dropped during unwinding, the mutex marks itself poisoned, and later `lock()`
calls return `Err(PoisonError)`. It protects against silently using state that a panic left mid-update. Options:
propagate (`lock().unwrap()` panics too, taking the failure to the next boundary), recover (`into_inner()` or
`unwrap_or_else(PoisonError::into_inner)` after checking or repairing the invariant, then `clear_poison()`), or treat it
as fatal for the component (stop serving it, restart).

**7. `UnwindSafe`.** A marker trait meaning "safe to observe after a caught panic". `&mut T` isn't `UnwindSafe` because
code after `catch_unwind` could see a `T` that the panic left half-updated. `AssertUnwindSafe` is a safe wrapper, not an
`unsafe` operation: getting it wrong yields logic bugs, never undefined behavior.

**8. Strategies.** Tokio gateway: `unwind`, so a panicking task becomes a `JoinError` and the process keeps serving other
tenants. JVM-hosted library: `unwind` plus `catch_unwind` at every exported function, converting panics to error codes;
under `abort`, any bug would kill the JVM. CLI: `abort`, because a bug should end the process immediately, binaries are
smaller, no half-updated state survives, and the hook still prints the message.

**9. `Drop` and failure.** A panic in `Drop` while unwinding aborts the process, and `Drop` has no way to return an error
anyway. Design cleanup that can fail as an explicit, consuming method returning `Result` (`commit`, `close`,
`sync_all`, `shutdown`), distinguishing failures with a known outcome from unknown outcomes, and keep `Drop` as a silent,
non-panicking backstop for every other path.

### Debugging exercise (`extern "C"` callback)

1. The abort happened inside `score` itself: frame 18 is `core::panicking::panic_cannot_unwind`, called from frame 19,
   `playground::score`. Since Rust 1.81 the compiler gives every `extern "C"` function an abort-on-unwind landing pad, so
   the unwind never reaches `catch_unwind` in frame 23.
2. The foreign frames above an `extern "C"` function have no Rust landing pads and expect no unwinding: their cleanup
   would be skipped (locks held, memory leaked) and, for a JVM or C code compiled without unwind tables, the behavior is
   undefined. A deterministic abort is the safe outcome.
3. Keep `extern "C"` and put `catch_unwind` **inside** the function, converting a panic into an error code (listing
   `ch03-06-ffi-boundary.rs`). Or declare `extern "C-unwind"` when every caller can handle an unwind (Rust callers, or
   C++ built with exceptions). For Meridian's JVM caller, only the first is acceptable.
4. It's a second panic while the first ("negative amount") is still in progress, and std's default hook prints a full
   backtrace for a panic that occurs during another one. The double-panic listing shows the same behavior.

### Selected exercises

- **Advanced:** a `Mutex` held by a panicking job is poisoned when its guard is dropped during unwinding. The pool
  survives (the job ran under `catch_unwind`), but the next job that locks the mutex gets `Err(PoisonError)` and must
  apply the lock's policy (propagate, recover, or treat as fatal).
- **Systems:** expect a roughly linear fit: a fixed cost (hook, allocation, starting the unwinder) of about a microsecond
  or two, plus a per-frame cost. With two guards, the landing pad drops them in reverse declaration order (the second
  guard first), and rustc may emit one pad per call site whose set of live values differs.

---

## Chapter 8.4 — Errors at Service Boundaries

### Interview & architecture questions

**1. Four questions.** Whose fault, did it happen, may it be retried, who must know. A declined card: the issuer refused
(client side), it didn't happen, never retry the same request, the client gets a 402 with `PAY_DECLINED`, logged at
INFO, counted as a business metric. A processor timeout: a dependency, **unknown** outcome, retry only with the same
idempotency key, 504 and `retryable: true`, logged at WARN, plus reconciliation. A failed ledger write: our side, unknown
or partial, no blind retry, 500 with `PAY_INTERNAL`, logged at ERROR with the full chain, alerts.

**2. Why timeouts are ambiguous.** A successful `write()` means the bytes reached the local kernel's send buffer, nothing
more. The request may have been delivered and processed, with only the response lost or late. Not ambiguous: connection
refused, connect timeouts, DNS failures, and TLS handshake failures before any request byte was sent (it didn't happen),
and any response that actually arrived, 4xx or 5xx (the server told you).

**3. Jitter.** Backoff spaces out one client's retries, but clients that failed at the same moment still retry at the
same moments, in synchronized waves that re-overload the recovering service. Full jitter randomizes each wait across
`[0, ceiling]`, spreading the load.

**4. Idempotency keys.** The client generates one key per logical operation, before the first attempt, and sends it on
every retry. The server checks the key before doing any work and stores `(key, request fingerprint, outcome)` atomically
with the effect, under a unique constraint. A retry with the same key and request gets the stored outcome; the same key
with a different request gets a conflict (409); a concurrent duplicate sees the entry in progress and gets 409 or waits.
Common mistakes: a new key per attempt (Chapter 8.4's debugging exercise), storing the key outside the transaction (two
duplicates both pass the check), and keys expiring before clients stop retrying.

**5. Amplification.** 3 × 3 × 3 = 27 attempts at the bottom per user action, arriving exactly while the bottom service is
failing. Prevent it by retrying at one layer only, and by bounding retries globally with a retry budget (token bucket) or
circuit breaker; propagating deadlines and honoring `Retry-After` help too.

**6. Exhaustive mapping.** A new variant forces a decision (E0004) in every classifier: status, code, class, log level.
A wildcard arm would silently put new failures into a default class: fraud rejections as retryable 500s (clients retry,
on-call is paged) or internal errors as 400s (nobody is paged).

**7. Client vs log.** Client: stable code, a safe message, `retryable`, and a request ID. Never file paths, SQL, hostnames,
stack traces, or panic messages: they leak internals (a security problem) and become a de facto API. Log: the full cause
chain, class, code, request ID, and context. Different audiences, different trust levels.

**8. Metric labels.** Safe: error code, class, route template, upstream name, status class, each with a small, bounded
set of values. Dangerous: user ID, request ID, error message, amount, card BIN, raw URL. Each distinct value creates a
new time series, which can exhaust the metrics system's memory or budget and cause an incident of its own.

**9. Fail open or closed.** Fail open when a false rejection costs more than the risk (fraud scoring down for small
payments, recommendations, non-critical enrichment). Fail closed when the risk is unacceptable (authentication, credit
limits, sanctions screening). The business and risk owners decide; engineers encode the decision per dependency and per
error class, and test it.

### Debugging exercise (key per attempt)

1. Each attempt generated a new key, so payments-core saw two distinct logical operations and correctly charged twice.
   The server can only deduplicate what the client identifies as the same operation.
2. One logical operation is "this checkout attempt of this order". Create the key when the checkout attempt starts,
   before the first network call, persist it with the checkout so it survives a gateway restart, and reuse it for every
   retry. A user clicking "Pay" twice should map to the same key (derive it from the order and checkout-attempt ID, and
   disable double submission in the UI); a deliberate second payment needs a new attempt and a new key.
3. A fake payments-core that records the key of every call and drops the first response; assert that all recorded keys
   are equal and that exactly one charge exists.
4. The idempotency key (plus the order ID) on every attempt's log line: two different keys for one order ID are visible
   immediately.

### Selected exercises

- **Beginner:** `FraudSuspected` is Rejected, with 402 (or 403/422, per the API's convention), code `PAY_FRAUD_SUSPECTED`,
  and a generic client message ("payment declined"). Don't return the score: it helps an attacker tune their attempts.
- **Intermediate:** predicted without a budget, with 4 attempts and 50% failure: 1 + 0.5 + 0.25 + 0.125 = 1.875 attempts
  per request, about 1,875 in total. With a 10% budget, retries are capped near 100, so about 1,100 attempts. Measure it.
- **Systems:** a read timeout on a Unix socket surfaces as `ErrorKind::WouldBlock` (std documents that the kind returned
  for a read timeout is platform-specific: `WouldBlock` on Unix, `TimedOut` on Windows); connecting to a closed port gives
  `ConnectionRefused`. The first is ambiguous, the second is safe to retry.

---

## Part VIII Review — Capstone: the refunds PR

**Defects** (fourteen; the principle in parentheses):

1. `req.amount.parse().unwrap()` panics on client input: a 500 for a typo, or a dead process under `panic = "abort"`.
   It should be a `400 REFUND_INVALID_AMOUNT` (8.1, 8.3).
2. **Negative and zero amounts are accepted** (one of the two non-error-handling defects): `"-500"` parses, the processor
   is asked to refund −500 (a charge), and the ledger's balance *increases*. Validate `amount > 0`.
3. `req.reason.unwrap_or_default()` silently accepts a missing reason. If refunds require a reason for audit, that's
   the "absent vs malformed" confusion; if they don't, the field should be `Option` all the way through (8.1).
4. `Processor::refund` returns `Result<String, String>`, and retryability is decided by `e.contains("503")`: stringly
   typed errors (8.2).
5. A non-idempotent refund is retried up to five times, with **no idempotency key**, no backoff, no jitter, and no
   deadline. Any ambiguous failure mislabeled as retryable refunds twice, and the tight loop hammers a struggling
   processor (8.4).
6. **When all retries fail, the loop falls through** with an empty `refund_id`: the ledger is debited and the client gets
   `200` with an empty ID. The customer is told they were refunded, the books say so, and no money moved.
7. `return (500, e)` sends the processor's message, including an internal host IP, to the client, and maps every
   processor failure, including definitive rejections, to 500 (8.4).
8. **Ordering:** the processor refunds *before* the ledger checks the refundable amount. If the ledger step then fails
   (not found, insufficient, storage error, or the panic in defect 10), money has left and the ledger doesn't know. See
   the ordering answer below.
9. `Ledger::debit` returns `Box<dyn Error>`: callers can't tell "no such payment" (404) from a storage failure (500), and
   without `+ Send + Sync` the error can't cross threads or be held across `.await` in a multi-threaded runtime (8.2).
10. `panic!("refund exceeds captured amount")` for a business condition that clients can trigger: a 500 at best; it
    poisons any lock held around it, and under abort it kills the process. It should be a variant mapped to 409 (8.3).
11. Log-and-return: `eprintln!("ERROR ...")` *and* returning the error, so the caller logs it again. Unstructured, no
    request ID, no code, ERROR level regardless of class (8.4 §10).
12. `format!("debit failed: {e:?}")` returns **Debug** output to the client: internal structure and an unstable format
    (8.2, 8.4).
13. `let _ = write_audit(...)` swallows the audit failure: refunds can happen with no audit record, which is a compliance
    problem that surfaces only in an audit (8.1 §10). Write the audit record in the same transaction as the refund (an
    outbox), or fail the request and alert.
14. **`write_audit` builds a file path from `payment_id`** (the second non-error-handling defect): a `payment_id` of
    `../../etc/cron.d/x` is a path traversal. And `fs::write` truncates, so a second refund on the same payment
    overwrites the first one's audit record.

**Severity.** Can move money incorrectly: 6, 8, 2, 5, 13. Can take down a process or poison shared state: 1, 10.
Security: 7, 14. Make operations and debugging harder: 11, 12, 4, 9, 3.

**Ordering.** Validate the request → check the idempotency key → **reserve** the amount in the ledger, recording a
pending refund with the key (and an outbox audit event) in one database transaction → call the processor with a key
derived from the client's → on success, mark the refund complete; on a definitive failure, release the reservation; on
an ambiguous failure, leave it pending for a reconciliation job that asks the processor what happened, by key. If the
process crashes between steps, the pending record with its key tells recovery exactly which step to resume: the same
role Chapter 8.3's `CommitError::OutcomeUnknown` plays inside one database.

**Signatures** (one defensible version, implemented in listing `review-02-refund-fixed.rs`):

```rust,ignore
#[derive(Debug, thiserror::Error)]
pub enum RefundError {
    InvalidAmount,                                  // 400
    PaymentNotFound(String),                        // 404
    ExceedsCaptured { requested: i64, refundable: i64 }, // 409
    IdempotencyConflict,                            // 409
    ProcessorUnavailable,                           // 503, retryable, reservation released
    ProcessorTimeout,                               // 504, retryable with the same key, reconciliation
}

pub enum ProcessorError { Unavailable, Timeout }   // classified at the client, never prose

impl Refunds {
    pub fn handle(&mut self, req: &RefundRequest, processor: &mut Processor) -> Result<String, RefundError>;
}
fn respond(result: Result<String, RefundError>, request_id: &str) -> String; // the one boundary: log + status + body
```

The redesign's verified output shows each class: 400, 404, and 409 responses at INFO; a request that survives two
`Unavailable` failures (three processor calls); a client retry (r5) that returns the same refund without calling the
processor; and a reused key rejected with 409. A production version would add a storage variant (500, Internal, logged
at ERROR with its chain), backoff with jitter, and the reconciliation job.

---

## Part VIII Review — Interview mode

1. **Java vs Go vs Rust.** Java checked exceptions make failures visible in signatures but don't compose with lambdas
   (so code wraps them in unchecked ones); unchecked exceptions are invisible. Go makes the check visible at every call
   but ignorable, and returns a value alongside the error. `Result` is visible, typed, and can't be used without
   deciding; being a value, it composes through closures, iterators, and futures. See Chapter 8.1 §8.
2. **`?`:** Chapter 8.1, questions 2 and 3.
3. **Choosing:** Chapter 8.1, question 1, and Chapter 8.3, question 1. Examples: `Option` for a cache lookup, `Result`
   for parsing a request, `Result<Option<_>, _>` for a database lookup, a panic for a broken ledger invariant.
4. **Emitted code:** one `test` and one predicted branch per `?` (Chapter 8.1 §6); a landing pad after the `ret`, reached
   through tables (Chapter 8.3, question 3).
5. **Aborts:** Chapter 8.3, question 4. Each exists because continuing would mean two in-flight panics, unwinding through
   frames that can't be unwound, or needing memory that doesn't exist.
6. **Exit statuses:** 1 for `main` returning `Err`; 101 for a panic unwinding out of `main`; `SIGABRT` (signal 6,
   reported as 134 by shells and Kubernetes) for an abort. All verified by listing `ch03-11-exit-status.rs`.
7. **Large errors:** Chapter 8.2, question 6.
8. **`Box<dyn Error>` / `anyhow`:** one allocation per error (plus one per context layer), none on success. It matters
   when failures are frequent on a hot path (a parser rejecting much of its input), not at typical service error rates.
9. **Panics for validation:** measure the failure-path cost (panic plus `catch_unwind` vs returning `Err`) at realistic
   stack depths and failure rates. Expect microseconds per failure vs nanoseconds (Chapter 8.3 measured about 1.8 µs vs
   3 ns at depth 1), plus lock poisoning and the risk of aborts. Fix the design, not the numbers.
10. **Library vs application:** Chapter 8.2, question 3. The real line is "will code branch on this error?"
11. **Panic strategy per system:** Chapter 8.3, question 8, and §9's hook-plus-boundary pattern.
12. **Fallible cleanup:** Chapter 8.3, question 9, with `CommitError::{RolledBack, OutcomeUnknown}`: "outcome unknown"
    must say "reconcile before retrying", and must consume the guard.
13. **Timeouts and idempotency:** Chapter 8.4, questions 2 and 4.
14. **Retry amplification:** Chapter 8.4, question 5.
15. **Internal errors and metrics:** Chapter 8.4, questions 7 and 8.
