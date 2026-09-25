# Chapter 8.4 — Errors at Service Boundaries

> **Where this sits:** Part VIII · Error Handling · chapter 4 of 4
> **Prerequisites:** Chapters 8.1–8.3. Some familiarity with HTTP status codes.
> **After this chapter you can:** classify any failure by whose fault it is, whether it happened, and whether it may be
> retried; turn a domain error enum into HTTP statuses, stable error codes, and safe client bodies, with the compiler
> checking that every variant is classified; design retries with backoff, jitter, deadlines, and `Retry-After`;
> implement idempotency keys and explain the bug they prevent; explain why a timeout is ambiguous down to the socket; and
> decide what to log, at which level, and what to count.

---

## Pass 1 · User level — *Errors leave the process*

### 1. Problem

Inside one process, an error is a typed value, and the caller matches on it. At a **service boundary** it has to become
something else:

- an HTTP status and a JSON body, read by another team's code, maybe written in Java or Go;
- a **retry decision**, made by a client library you don't control;
- a **log line**, read by someone at 3 a.m.;
- a **metric**, which may page someone.

Most error-handling *outages* happen here, not inside functions: retry storms that flatten a recovering service, double
charges from retried payments, on-call engineers paged for declined cards while a real disk failure scrolls past, stack
traces with internal hostnames returned to customers. This chapter builds the error boundary of Meridian's
**payments-core**, the new Rust service that orchestrates card charges between the gateway and the card processor (the
Java ledger stays behind it).

### 2. Mental model

**Four questions every error must answer at a boundary:**

```text
 1. WHOSE FAULT?       the client's request · our code · a dependency
 2. DID IT HAPPEN?     no · yes · UNKNOWN
 3. RETRY?             never · later, with backoff · only with the same idempotency key
 4. WHO MUST KNOW?     the client (status + code) · the on-call (log level, alert) · the dashboard (metric)
```

The answers group into four **classes**, which is the taxonomy this chapter uses:

| Class | Did it happen? | Retry? | HTTP | Log level | Payments examples |
|---|---|---|---|---|---|
| **Rejected** | No, and it won't | Never (same request, same answer) | 4xx | INFO | invalid amount, card declined, idempotency key reused |
| **Transient** | No | Yes: backoff, jitter, honor `Retry-After` | 429, 503 | WARN | rate limited, processor unavailable |
| **Ambiguous** | **Unknown** | Only with the same idempotency key | 504 | WARN | processor timed out after the request was sent |
| **Internal** | Unknown or partial | Not blindly: a human must look | 500 | ERROR | ledger write failed, a panic (Chapter 8.3) |

**The Ambiguous row is the one that distinguishes distributed systems from local ones.** A local function either
returned or didn't. A remote call can time out *after* the other side did the work. Everything in §4–§6 follows from
taking that row seriously.

**The boundary pipeline:**

```text
 PaymentError (a library-style enum, Chapter 8.2)           ← inner layers add context and RETURN; they don't log
      │  classified ONCE: class(), code(), http_status()
      ├──► client:  status + {"code", "message", "retryable", "request_id"}   (no internals, ever)
      ├──► log:     ONE line, level by class, full cause chain, request_id
      └──► metric:  payment_errors_total{code, class} += 1                      (low-cardinality labels only)
```

### 3. Rust code

The payments domain error and its classification (listing `ch04-01-taxonomy.rs`, verified):

```rust
use std::error::Error;
use std::io;

/// Meridian payments: the domain error. One enum, one place where every failure is classified.
#[derive(Debug, thiserror::Error)]
pub enum PaymentError {
    #[error("amount must be positive, got {0}")]
    InvalidAmount(i64),
    #[error("card declined: {reason}")]
    Declined { reason: &'static str },
    #[error("idempotency key reused with a different request")]
    IdempotencyConflict,
    #[error("rate limited, retry after {retry_after_ms} ms")]
    RateLimited { retry_after_ms: u64 },
    #[error("card processor unavailable")]
    ProcessorUnavailable,
    #[error("card processor timed out")]
    ProcessorTimeout,
    #[error("ledger write failed")]
    Ledger(#[source] io::Error),
}

/// What the failure means for the caller, which is what the caller actually needs to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// The request is wrong or refused. The same request will fail the same way: don't retry.
    Rejected,
    /// It did not happen, and may succeed later: retry with backoff.
    Transient,
    /// We don't know whether it happened: retry ONLY with the same idempotency key.
    Ambiguous,
    /// Our bug or a broken dependency invariant: don't retry blindly, alert a human.
    Internal,
}

impl PaymentError {
    pub fn class(&self) -> Class {
        match self {
            Self::InvalidAmount(_) | Self::Declined { .. } | Self::IdempotencyConflict => Class::Rejected,
            Self::RateLimited { .. } | Self::ProcessorUnavailable => Class::Transient,
            Self::ProcessorTimeout => Class::Ambiguous,
            Self::Ledger(_) => Class::Internal,
        }
    }

    pub fn http_status(&self) -> u16 {
        match self {
            Self::InvalidAmount(_) => 400,
            Self::Declined { .. } => 402,
            Self::IdempotencyConflict => 409,
            Self::RateLimited { .. } => 429,
            Self::ProcessorUnavailable => 503,
            Self::ProcessorTimeout => 504,
            Self::Ledger(_) => 500,
        }
    }

    /// Stable, machine-readable, documented. Messages may change; codes may not.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidAmount(_) => "PAY_INVALID_AMOUNT",
            Self::Declined { .. } => "PAY_DECLINED",
            Self::IdempotencyConflict => "PAY_IDEMPOTENCY_CONFLICT",
            Self::RateLimited { .. } => "PAY_RATE_LIMITED",
            Self::ProcessorUnavailable => "PAY_PROCESSOR_UNAVAILABLE",
            Self::ProcessorTimeout => "PAY_PROCESSOR_TIMEOUT",
            Self::Ledger(_) => "PAY_INTERNAL",
        }
    }

    /// What leaves the service. Internal details (file paths, SQL, stack traces) never do.
    pub fn client_body(&self, request_id: &str) -> serde_json::Value {
        let message = match self.class() {
            Class::Internal => "internal error".to_string(),
            _ => self.to_string(),
        };
        let mut body = serde_json::json!({
            "code": self.code(),
            "message": message,
            "retryable": matches!(self.class(), Class::Transient | Class::Ambiguous),
            "request_id": request_id,
        });
        if let Self::RateLimited { retry_after_ms } = self {
            body["retry_after_ms"] = (*retry_after_ms).into();
        }
        body
    }
}

fn main() {
    let errors = [
        PaymentError::InvalidAmount(-500),
        PaymentError::Declined { reason: "insufficient_funds" },
        PaymentError::IdempotencyConflict,
        PaymentError::RateLimited { retry_after_ms: 250 },
        PaymentError::ProcessorUnavailable,
        PaymentError::ProcessorTimeout,
        PaymentError::Ledger(io::Error::other("disk quota exceeded on /var/lib/ledger/wal-000017")),
    ];
    for e in &errors {
        println!("{:<26} {:<10} {}", e.code(), format!("{:?}", e.class()), e.http_status());
    }
    let internal = errors.last().unwrap();
    println!("log:    {internal} | cause: {}", internal.source().unwrap());
    println!("client: {}", internal.client_body("req-7f3a"));
    println!("client: {}", errors[3].client_body("req-7f3b"));
}
```

```text
PAY_INVALID_AMOUNT         Rejected   400
PAY_DECLINED               Rejected   402
PAY_IDEMPOTENCY_CONFLICT   Rejected   409
PAY_RATE_LIMITED           Transient  429
PAY_PROCESSOR_UNAVAILABLE  Transient  503
PAY_PROCESSOR_TIMEOUT      Ambiguous  504
PAY_INTERNAL               Internal   500
log:    ledger write failed | cause: disk quota exceeded on /var/lib/ledger/wal-000017
client: {"code":"PAY_INTERNAL","message":"internal error","request_id":"req-7f3a","retryable":false}
client: {"code":"PAY_RATE_LIMITED","message":"rate limited, retry after 250 ms","request_id":"req-7f3b","retry_after_ms":250,"retryable":true}
```

The log line has the file path; the client body doesn't. The client gets a `request_id` instead, which support staff
can use to find the log line. `retryable` tells generic client code what to do without knowing every code.

**The compiler keeps the taxonomy complete.** Every classifier is a `match` with no wildcard. When a later release adds
a variant, every classifier stops compiling until someone decides what the new failure means (listing
`ch04-02-new-variant.rs`):

```rust,compile_fail
#[derive(Debug)]
pub enum PaymentError {
    InvalidAmount(i64),
    Declined { reason: &'static str },
    ProcessorTimeout,
    FraudSuspected { score: u8 }, // added in a later release
}

impl PaymentError {
    pub fn http_status(&self) -> u16 {
        match self {
            Self::InvalidAmount(_) => 400,
            Self::Declined { .. } => 402,
            Self::ProcessorTimeout => 504,
        }
    }
}
```

```text
error[E0004]: non-exhaustive patterns: `&PaymentError::FraudSuspected { .. }` not covered
  --> src/main.rs:12:15
   |
12 |         match self {
   |               ^^^^ pattern `&PaymentError::FraudSuspected { .. }` not covered
```

This is Chapter 2.5's ledger chargeback lesson applied to errors: a wildcard `_ => 500` would have compiled, and
fraud rejections would have been reported as internal errors, retried by clients, and paged on-call.

---

## Pass 2 · Systems level — *Retries, idempotency, and what a timeout means*

### 4. Under the hood

**What the protocol already says.** HTTP semantics (RFC 9110) separate client errors (4xx: "don't repeat this request
without changing it") from server errors (5xx). `429 Too Many Requests` (RFC 6585) and `503 Service Unavailable` may
carry a `Retry-After` header. Methods are **idempotent** or not: `GET`, `HEAD`, `PUT`, and `DELETE` may be repeated
safely by definition, so clients and proxies retry them automatically. `POST` may not, which is why a charge needs more
than HTTP gives it. gRPC's status codes encode the same classes: `INVALID_ARGUMENT` and `FAILED_PRECONDITION` (Rejected),
`UNAVAILABLE` and `RESOURCE_EXHAUSTED` (Transient), `DEADLINE_EXCEEDED` (Ambiguous: the server may have finished the
work), and `INTERNAL`.

**Retries with backoff and jitter.** Retrying immediately turns a struggling dependency into a crushed one. The
standard recipe is **exponential backoff with full jitter**: before attempt *n*, sleep a random time in
`[0, min(cap, base × 2ⁿ)]` (described in Marc Brooker's "Exponential Backoff And Jitter", AWS Architecture Blog, 2015).
The randomness spreads out clients that failed at the same moment, so they don't retry in synchronized waves. The core of
listing `ch04-03-retry.rs` (time is simulated so the run is deterministic):

```rust,ignore
/// Exponential backoff with full jitter: sleep a random time in [0, min(cap, base * 2^attempt)].
fn backoff(p: &Policy, attempt: u32, rng: &mut Rng) -> Duration {
    let ceiling = p.base.saturating_mul(1 << attempt.min(16)).min(p.cap);
    ceiling.mul_f64(rng.next_f64())
}

// inside the retry loop, after attempt `attempt` failed with `err`:
let retryable = match err.class() {
    Class::Rejected => false,
    Class::Transient => true,
    Class::Ambiguous => idempotent,          // only if a replay can't do the work twice
};
if !retryable || attempt + 1 == p.max_attempts {
    return (Err(err), log);                  // give up
}
let mut wait = backoff(p, attempt, &mut rng);
if let CallError::RateLimited { retry_after } = err {
    wait = wait.max(retry_after);            // the server told us when: listen
}
if elapsed + wait > p.budget {
    return (Err(err), log);                  // never sleep past the caller's deadline
}
```

With `base` 50 ms, `cap` 1 s, 4 attempts, and an 800 ms deadline budget (sleeps shown rounded to milliseconds):

```text
flaky processor: Ok("ch_123")
    attempt 0: Unavailable → sleep 42ms (t=42ms)
    attempt 1: Unavailable → sleep 39ms (t=82ms)
declined: Err(Declined)
    attempt 0: Declined → give up (Rejected)
timeout, no idempotency key: Err(Timeout)
    attempt 0: Timeout → give up (Ambiguous)
timeout, with idempotency key: Ok("ch_124")
    attempt 0: Timeout → sleep 42ms (t=42ms)
rate limited: Err(RateLimited { retry_after: 600ms })
    attempt 0: RateLimited { retry_after: 300ms } → sleep 300ms (t=300ms)
    attempt 1: RateLimited { retry_after: 600ms } → would wait 600ms, past the deadline budget: give up
```

Every branch of the taxonomy is exercised. A decline is never retried. A timeout is retried **only** when the call
carries an idempotency key. And a rate-limited call gives up early, because waiting as long as the server asked would
exceed the caller's deadline. Returning a clean error at 300 ms is better than a success at 900 ms that the user has
already abandoned.

**Idempotency keys.** An idempotency key turns "at least once" delivery into "exactly once" *effect*. The client
generates a unique key for one **logical** operation, before the first attempt, and sends the same key with every retry.
The server stores, per key, a fingerprint of the request and its outcome (listing `ch04-04-idempotency.rs`):

```rust,ignore
fn charge(&mut self, key: &str, amount: i64) -> Result<Charge, ChargeError> {
    if let Some(entry) = self.keys.get(key) {
        return match entry {
            Entry::Done { amount: a, result } if *a == amount => result.clone(), // replay the original outcome
            Entry::InProgress { amount: a } if *a == amount => Err(ChargeError::InProgress), // concurrent duplicate
            _ => Err(ChargeError::IdempotencyConflict), // same key, different request: a client bug
        };
    }
    self.keys.insert(key.to_string(), Entry::InProgress { amount });
    let result = self.charge_card(amount); // in production: the processor call carries the key too
    self.keys.insert(key.to_string(), Entry::Done { amount, result: result.clone() });
    result
}
```

The gateway sends a charge, payments-core performs it, and the **response** is lost on the way back. The gateway sees a
timeout and retries:

```text
naive:      first=Err(Timeout) retry=Ok(Charge { id: "ch_2", amount: 4999 }) charges=2
idempotent: first=Err(Timeout)
            retry=Ok(Charge { id: "ch_1", amount: 4999 })
            reuse=Err(IdempotencyConflict)
            charges=1
```

Without a key, the customer paid twice, and the gateway saw nothing wrong: one timeout, one success. With a key, the
retry returns the **original** charge (`ch_1`), and a client bug that reuses the key for a different amount gets
`409 PAY_IDEMPOTENCY_CONFLICT` instead of a silent replay of the wrong charge. Three details matter in production:

- **Store the key in the same transaction as the effect**, with a unique constraint on `(tenant, key)`, so two
  concurrent duplicates can't both pass the check. The in-memory map here models that constraint.
- **Handle in-progress duplicates.** A retry can arrive while the first attempt is still running. Return 409 (or wait),
  never "not found, go ahead".
- **Propagate the key downstream.** payments-core passes a key to the card processor too, so its own retries are safe.
  Stripe's API popularized the `Idempotency-Key` request header, and an IETF HTTP API working-group draft standardizes it
  (in draft at the time of writing).

### 5. Memory

**The idempotency store is a capacity-planning problem.** Arithmetic, not a measurement: if every ledger write were keyed
at Meridian's ~3K TPS and keys were kept for 24 hours, that's 3,000 × 86,400 ≈ **259 million entries**. At roughly 200
bytes each (a 36-byte UUID key, a 32-byte SHA-256 request fingerprint, a stored response, and index overhead), that's
about **52 GB**. Three consequences:

- It doesn't belong in a process-local `HashMap`: it must survive restarts and be shared by all replicas. It's a
  database table (or a replicated cache with expiry) with a unique index.
- **Retention is a contract.** Keys expire, so the API must document the window ("retries with the same key are
  deduplicated for 24 hours"), and clients must not retry beyond it.
- **Store a fingerprint, not the request.** A hash of the canonical request body detects key reuse without storing
  card data you'd then have to protect.

**Error values on the hot path stay small.** `PaymentError` is a few words: the largest variant holds an `io::Error`
(8 bytes). The cause chain is rendered into a `String` only when a log line is actually emitted (§10), and client bodies
are built only for failed requests. Chapter 8.2's rules still apply at the boundary: no 512-byte context buffers in
the error, no backtrace capture for Rejected errors.

### 6. CPU / OS

**Why a timeout is ambiguous, down to the socket.** [OS] When `write()` on a TCP socket returns successfully, the bytes
have been copied into the **local** kernel's send buffer. That says nothing about whether the peer received them, and
even less about whether the peer's application *processed* them. A request-response exchange that fails can fail at
different points, and they mean different things:

```text
 gateway                                   payments-core
    │── connect() ── SYN ─────────────────────►│   refused / unreachable here → request NEVER SENT → safe to retry
    │◄──────────────────────────── SYN-ACK ────│
    │── write(request) ───────────────────────►│   bytes in OUR kernel buffer; peer may not have them yet
    │                                          │── charge card, commit ──►   the effect HAPPENS here
    │         ✗ response lost / slow           │
    │── read() ... timeout fires               │   → AMBIGUOUS: the charge may exist
```

How that surfaces in Rust [LIB] [OS]:

| `io::ErrorKind` | Typical errno | Phase | Did the server get the request? |
|---|---|---|---|
| `ConnectionRefused` | `ECONNREFUSED` | connect | No: nothing was sent. Safe to retry. |
| `TimedOut` (connect) | `ETIMEDOUT` | connect | No |
| `TimedOut` / `WouldBlock` (read) | `EAGAIN` with a socket read timeout, or a runtime timer | awaiting response | **Unknown** |
| `ConnectionReset` | `ECONNRESET` | after sending | **Unknown** |
| `BrokenPipe` | `EPIPE` | while sending | Unknown unless you know no bytes were sent |

So a client can classify a failure only if it knows **which phase** it was in, which is why HTTP client libraries
distinguish connect-phase errors from the rest. A generic "retry on any `io::Error`" policy is wrong for anything that
isn't idempotent.

**Retries multiply load.** Arithmetic: if the gateway, payments-core, and the processor client each retry up to 3 times,
one user action can become 3 × 3 × 3 = **27** attempts at the processor, and they arrive exactly when the processor is
failing. That's how a brief slowdown becomes an outage. §7 gives the rules that prevent it.

---

## Pass 3 · Architect level — *Designing the boundary*

### 7. Trade-offs

**Retry at one layer.** Pick the layer that knows whether the operation is idempotent, usually the one closest to the
user that holds the idempotency key. Every other layer fails fast and reports the class upward. Chapter 21.4 covers
timeouts, retries, and circuit breakers as network mechanisms.

**Bound retries globally, not just per call.** A **retry budget** caps retries at a fraction of normal traffic (for
example, "retries may add at most 10% to request volume"), so during an outage retries stop instead of multiplying.
gRPC's retry throttling and client-side circuit breakers implement the same idea. **Propagate deadlines**: pass the
remaining time budget downstream, and never start an attempt that can't finish inside it (the rate-limited case in §4).

**Fail open or fail closed?** When a dependency fails, the class says what happened; the business decides what to do.
If fraud scoring is unavailable, payments-core can approve small payments (fail open: revenue over risk) or decline
everything (fail closed: risk over revenue). Encode the choice explicitly per dependency and per error class, and test
it. Don't let it fall out of whatever `unwrap_or` happened to be written.

**Codes are API; messages are not.** Clients branch on `code` (and `retryable`), which you version and document like any
endpoint. Messages are for humans and may change (Chapter 8.2's failure scenario is what happens when clients parse
them).

**Detail vs security.** Rejected errors can say exactly what's wrong with the request ("amount must be positive, got
-500"). Internal errors say "internal error" plus a `request_id`. File paths, SQL, hostnames, and panic messages go to
logs. The review capstone has a PR that returns a processor's host IP to the client.

**Status-code granularity is secondary.** Whether a decline is 402 or 422 is debated. What matters is that it's a 4xx
(don't retry, don't page) with a stable code. Pick a convention, document it, and keep it.

### 8. Java comparison

| Java / Go | Rust | Notes |
|---|---|---|
| Spring `@ControllerAdvice` + `@ExceptionHandler(DeclinedException.class)` | One mapping from `PaymentError` to a response (`http_status`, `code`, `client_body`) | Spring picks a handler by exception type at run time; unmapped exceptions fall to a generic 500 handler. Rust's mapping is an exhaustive `match`: an unmapped variant is E0004. |
| Resilience4j `Retry`, `CircuitBreaker`, `TimeLimiter` | `tower` layers (`Retry` with a policy, `Timeout`, concurrency limits); Chapter 22.3 | Same patterns. A tower retry policy decides per error value, so it can use `class()`. |
| Go `errors.Is(err, context.DeadlineExceeded)` | matching `ProcessorTimeout`, or `io::ErrorKind::TimedOut` | |
| Go `net/http`'s automatic retry of idempotent requests on a reused connection | Your client's policy | Go's `Transport` treats a request carrying an `Idempotency-Key` header as replayable: the header is recognized at the protocol-library level. |

In axum (not on the Playground, so not verified here; Chapter 22.3 covers it), the mapping becomes an
`impl IntoResponse for PaymentError` that calls `http_status()` and `client_body()`, and handlers return
`Result<Json<Charge>, PaymentError>`: `?` inside the handler, one mapping at the edge.

> **Analogy limit.** "`impl IntoResponse for PaymentError` is `@ExceptionHandler`" is true for *where* the mapping
> lives and false for *what it covers*. Spring's handler catches by type, including exceptions nobody declared (an NPE
> becomes a 500 through the generic handler). The Rust mapping covers exactly the variants of one closed enum, and the
> compiler proves it covers all of them. Everything outside it (a panic) is handled by a separate boundary (Chapter 8.3),
> not by a catch-all.

### 9. Production scenario

**The double charge, and payments-core's error boundary.** In March 2025, a processor slowdown pushed Meridian's
Java payments responses past the 2-second read timeout the gateway uses for the payments upstream. The gateway's
generic HTTP client retried failed requests, including `POST /charges`. For 40 minutes, every slow charge was performed
twice: 1,140 customers were double-charged before the retry was disabled. The mechanism is exactly the `naive` line in §4: one timeout, one success,
two charges.

The redesign, built into the new Rust payments-core:

1. **Idempotency keys are mandatory** on `POST /charges`. The gateway generates one UUID per checkout attempt *before*
   the first try and sends it with every retry. payments-core stores `(tenant, key) → (fingerprint, outcome)` in the
   same database transaction as the charge record, and forwards a derived key to the processor.
2. **One retry layer.** The gateway retries only responses whose `retryable` is `true`, only with the same key, with
   full jitter and a deadline budget. payments-core doesn't retry the processor itself; the processor client fails fast
   with a classified error.
3. **The taxonomy is the contract.** §3's table is published in the API docs: status, code, class, and the retry rule
   for each. Adding a variant is a compile error in every classifier (listing `ch04-02-new-variant.rs`) and an API-docs
   change in the same PR.
4. **Ambiguous is surfaced, not hidden.** A `PAY_PROCESSOR_TIMEOUT` with no key-based replay available triggers a
   reconciliation job that asks the processor what happened, the distributed-systems version of Chapter 8.3's
   `CommitError::OutcomeUnknown`.

### 10. Failure scenario

**The 4xx page storm.** The Java payments service logged every failed payment at ERROR, including declines. Alerting
fired on the ERROR-log rate. On Black Friday, a surge of declined cards (expired cards and insufficient funds, the system
working correctly) produced 40,000 ERROR lines per minute. On-call was paged, looked at declines, and muted the alert.
Twenty minutes later the ledger host started failing `fsync` with `EIO`, a real Internal failure, and its ERROR lines
were buried in the decline noise until a reconciliation mismatch surfaced the next morning.

payments-core logs and counts by class, once, at the boundary (listing `ch04-05-observability.rs`, using `tracing`):

```rust,ignore
fn record(e: &PaymentError, request_id: &str, metrics: &mut BTreeMap<(&'static str, &'static str), u64>) {
    *metrics.entry((e.code(), e.class())).or_default() += 1;
    match e.class() {
        // A declined card is the system working correctly: not an ERROR, never pages anyone.
        "rejected" => tracing::info!(request_id, code = e.code(), class = e.class(), "payment rejected"),
        "transient" => tracing::warn!(request_id, code = e.code(), class = e.class(), error = %e, "payment failed, retryable"),
        _ => tracing::error!(request_id, code = e.code(), class = e.class(), error = %chain(e), "payment failed"),
    }
}
```

```text
 INFO payment rejected request_id="req-01" code="PAY_DECLINED" class="rejected"
 WARN payment failed, retryable request_id="req-02" code="PAY_PROCESSOR_UNAVAILABLE" class="transient" error=card processor unavailable
 INFO payment rejected request_id="req-03" code="PAY_DECLINED" class="rejected"
ERROR payment failed request_id="req-04" code="PAY_INTERNAL" class="internal" error=ledger write failed: fsync failed: EIO
payment_errors_total{code="PAY_DECLINED",class="rejected"} 2
payment_errors_total{code="PAY_INTERNAL",class="internal"} 1
payment_errors_total{code="PAY_PROCESSOR_UNAVAILABLE",class="transient"} 1
```

(`tracing_subscriber::fmt()` with timestamps, targets, and colors turned off so the output is reproducible. The metric
lines use the Prometheus text format, printed by hand.) The rules this encodes:

- **Level by class.** Rejected is INFO (or DEBUG): the system worked. Transient is WARN. Only Internal is ERROR.
- **Alert on symptoms and on Internal**, not on log volume: the rate of `class="internal"`, and SLO burn (the fraction of
  requests failing for reasons that are *our* fault). A decline spike shows up as a business metric, not a page.
- **Log once, at the boundary.** Inner layers add context (`.context()`, `#[source]`) and return. Logging at every layer
  ("log and rethrow") turns one failure into five lines, and the counts stop meaning anything.
- **Full chain in the log, never in the response.** `chain(e)` renders "ledger write failed: fsync failed: EIO" for the
  log; the client got `PAY_INTERNAL` and a request ID.
- **Metric labels are low-cardinality.** `code` and `class` have a handful of values. An error *message*, an amount, a
  card BIN, or a user ID as a label creates a new time series per value and can take down the metrics system.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part VIII).*

1. What four questions must an error answer at a service boundary? Classify a declined card, a processor timeout, and
   a failed ledger write.
2. Why is a timeout "ambiguous"? Explain it at the TCP level, and say which socket errors are *not* ambiguous.
3. Describe exponential backoff with full jitter. What problem does the jitter solve that backoff alone doesn't?
4. How does an idempotency key make a retried `POST` safe? Where must the key be generated, stored, and checked, and
   what happens with a concurrent duplicate?
5. Three layers each retry three times. What happens during an outage, and what are two mechanisms that prevent it?
6. Why must the error-to-status mapping be an exhaustive `match`? What would a wildcard arm cost you?
7. What goes into the client response for an Internal error, and what goes into the log? Why the difference?
8. Which labels are safe on an error metric? Give an example of a label that would cause an incident.
9. When should a dependency failure fail open, and when fail closed? Who decides?

### 12. Exercises

- **Beginner.** Add a `FraudSuspected { score: u8 }` variant to listing `ch04-01-taxonomy.rs`. Decide its class, status,
  code, and client message, and fix every E0004 the compiler reports. Should the client message include the score?
- **Intermediate.** Extend listing `ch04-03-retry.rs` with a **retry budget**: a token bucket that earns 0.1 token per
  first attempt and spends 1 per retry. Simulate 1,000 requests against a dependency that fails 50% of the time, with
  and without the budget, and count total attempts.
- **Advanced.** Make listing `ch04-04-idempotency.rs` safe under concurrency: wrap the store in a `Mutex`, run two
  threads that send the same key at once, and show that exactly one charge happens and the other request gets
  `InProgress` or the replayed result. Then add expiry, and show a retry after expiry creating a second charge. What
  must the API documentation say?
- **Systems.** On the Playground, open a `TcpListener` on localhost, accept a connection, read the request, and never
  reply. Give the client a read timeout and print the `io::ErrorKind` it gets. Then close the listener *before*
  connecting and print that error. Map each to the table in §6.
- **Architecture.** Draw the error boundary of a service you know: which layer classifies, which logs, which retries,
  what the client sees for each class. Find one place where a failure is logged twice and one where it is retried at two
  layers.

### 13. Debugging exercise

A teammate adds idempotency keys to the gateway's retry loop. payments-core deduplicates by key correctly. Support still
reports a double charge (listing `ch04-06-key-per-attempt.rs`):

```rust,ignore
let mut next = 0;
let mut new_key = || {
    next += 1;
    format!("idem-{next}")
};
// The gateway's retry loop. The first response is lost in the network.
for attempt in 0..3 {
    let key = new_key();
    let result = payments.charge(&key).and_then(|n| if attempt == 0 { Err(ChargeError::Timeout) } else { Ok(n) });
    println!("attempt {attempt} key={key} -> {result:?}");
    if result.is_ok() {
        break;
    }
}
println!("charges={}", payments.charges);
```

```text
attempt 0 key=idem-1 -> Err(Timeout)
attempt 1 key=idem-2 -> Ok(2)
charges=2
```

1. The server's idempotency logic is correct. Why was the customer charged twice?
2. What does "one logical operation" mean here? Where in the gateway should the key be created, and what's its
   lifetime? What if the *user* clicks "Pay" twice?
3. Write a test that would have caught this, using only the gateway's retry loop and a fake payments-core.
4. The logs showed two different keys for one checkout. What log field would have made this obvious in minutes?

### 14. Design exercise

**Ferrite v2's error protocol.** Ferrite v2 (Project L5) serves the v1 line protocol over Tokio, with a connection limit,
per-connection backpressure, timeouts, and graceful shutdown. Its only error response is `-ERR <message>`.

- Design error codes as the first token after `-ERR` (Redis does this: `-WRONGTYPE ...`). Which conditions need their
  own code: unknown command, value too large, server busy (connection limit), shutting down, timeout, internal error?
- Classify each code (Rejected, Transient, Ambiguous, Internal). Which may a client retry automatically?
- `SET k v` and `DEL k` are idempotent; would an `INCR k` command be? What does a client do when an `INCR` times out,
  and what would Ferrite need to make it safe?
- During graceful shutdown, what should in-flight and new requests receive? How does a client tell "shutting down,
  try another node" apart from "internal error"?
