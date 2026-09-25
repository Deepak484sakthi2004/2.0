# Part VIII Review — The Refunds PR & Interview Mode

> Consolidate Part VIII, then use it: review a pull request that compiles, looks reasonable, and contains at least a
> dozen error-handling defects, several of which move money. Then answer senior-level questions without notes.
> Answers are in **Appendix A, Part VIII**.

---

## Part VIII on one page

```text
 VALUES           Option = absence is normal · Result = failure with a reason · Result<Option<T>,E> = lookup that can fail
                  expr?  ≡  match Try::branch(expr) { Continue(v) => v, Break(r) => return FromResidual::from_residual(r) }
                  for Result, from_residual = Err(From::from(e))   (verified MIR; Try is unstable)
                  release asm: one `test` + one predicted branch per `?`; the error path is a few bit operations
        │
 TYPES            an error type is an API: variants for PROGRAMS, Display for PEOPLE, source() links layers
                  chain rule: a layer's Display never repeats its cause
                  library → precise enum (thiserror); application → anyhow + context; branch? then enum
                  cost: size paid on EVERY return (Result<u64, 516-byte E> = 520 B, sret + memcpy per `?` layer);
                        Box<dyn Error>/anyhow = 1 allocation per error, 0 on success
        │
 BUGS             panic = a bug; hook runs first; unwind runs drops via landing pads (zero-cost until thrown)
                  boundaries: catch_unwind (request), JoinError (Tokio task), catch_unwind + codes (FFI entry)
                  aborts: panic="abort", panic in Drop while unwinding, unwinding out of extern "C" (1.81+), OOM
                  poisoning reports half-done updates; Drop must never panic; fallible cleanup is an explicit method
                  cost: 1.8–3.2 µs per caught panic vs 3–16 ns per returned Err (one run, noisy)
        │
 BOUNDARIES       whose fault? did it happen? retry? who must know?  →  Rejected · Transient · Ambiguous · Internal
                  exhaustive mapping to status + stable code; client gets code + request_id, logs get the chain
                  retry at ONE layer: backoff + full jitter + deadline budget + Retry-After + retry budget
                  timeouts are ambiguous → idempotency keys, created once per logical operation, stored with the effect
                  log once at the boundary, level by class; metrics labeled by code/class only
```

## Ten ideas to carry forward

1. **Absence is not failure, and malformed is not absent.** `Option` and `Result` encode that difference; defaults
   belong to the first only.
2. **`?` is a visible, typed, early return** with a `From` conversion. It isn't an exception, and it costs a branch.
3. **An error type is an API** with two audiences. If code branches on it, it needs variants, not prose.
4. **Print each cause once.** `Display` describes this layer; `source()` points to the cause; a reporter walks the chain.
5. **Error size is paid on the success path.** Keep errors small or box the large ones.
6. **Panics are for bugs**, and every panic needs a boundary that decides how far it goes.
7. **`Drop` must never panic, and can't report failure.** Fallible cleanup is an explicit, consuming method; `Drop` is
   the backstop.
8. **"Did it happen?" has three answers.** Unknown outcomes (timeouts, lost commit acknowledgments) are the core problem
   of distributed error handling.
9. **Retry at one layer, with jitter, a deadline, a budget, and an idempotency key.** Anything else amplifies outages or
   duplicates effects.
10. **Classify once, at the boundary, exhaustively**, then derive the status, the code, the client body, the log level,
    and the metric from the class.

---

## Capstone: review the refunds PR

A teammate submits the refund endpoint for payments-core (listing `review-01-refund-pr.rs`; verified to compile as a
library crate). On the happy path it works.

```rust,ignore
use std::collections::HashMap;
use std::error::Error;

pub struct RefundRequest {
    pub payment_id: String,
    pub amount: String,
    pub reason: Option<String>,
}

pub struct Ledger {
    balances: HashMap<String, i64>,
}

pub struct Processor;

impl Processor {
    pub fn refund(&self, payment_id: &str, amount: i64) -> Result<String, String> {
        if payment_id.is_empty() { Err("processor: 503 upstream pool exhausted (host 10.2.7.14)".into()) } else { Ok(format!("rf_{amount}")) }
    }
}

impl Ledger {
    pub fn debit(&mut self, payment_id: &str, amount: i64) -> Result<i64, Box<dyn Error>> {
        let balance = self.balances.get_mut(payment_id).ok_or("no such payment")?;
        if *balance < amount {
            panic!("refund exceeds captured amount");
        }
        *balance -= amount;
        Ok(*balance)
    }
}

pub fn handle_refund(req: RefundRequest, ledger: &mut Ledger, processor: &Processor) -> (u16, String) {
    let amount: i64 = req.amount.parse().unwrap();
    let _reason = req.reason.unwrap_or_default();

    let mut refund_id = String::new();
    for _ in 0..5 {
        match processor.refund(&req.payment_id, amount) {
            Ok(id) => {
                refund_id = id;
                break;
            }
            Err(e) if e.contains("503") => continue,
            Err(e) => return (500, e),
        }
    }

    match ledger.debit(&req.payment_id, amount) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("ERROR debit failed: {e}");
            return (500, format!("debit failed: {e:?}"));
        }
    }

    let _ = write_audit(&req.payment_id, amount);
    (200, refund_id)
}

fn write_audit(payment_id: &str, amount: i64) -> std::io::Result<()> {
    std::fs::write(format!("/var/log/audit/{payment_id}"), amount.to_string())
}
```

**Your review.**

1. Find **at least twelve** defects. For each, name the Part VIII principle it violates and say what happens in
   production (which customer, which on-call engineer, or which auditor notices, and when).
2. Rank them by severity. Which ones can move money incorrectly? Which can take down a process? Which only make
   debugging harder?
3. There's an **ordering** problem that no error type fixes: the processor refunds *before* the ledger checks the
   refundable amount. Walk through what happens when the ledger step fails after the processor succeeded. Propose an
   order of operations, and say what state must be recorded if the process crashes between two steps.
4. Two defects are not error handling at all, but the error path exposes them. Find them. (Hint: one is in
   `write_audit`'s first line.)
5. Write the new signatures: the error enum(s), the processor client's error type, and `handle_refund`'s return type.

A redesign (listing `review-02-refund-fixed.rs`, verified) handles six requests, including a client retry and a reused
key, and prints:

```text
  [INFO ] request_id=r1 code=REFUND_INVALID_AMOUNT amount must be a positive whole number of cents
r1: 400 {"code":"REFUND_INVALID_AMOUNT","message":"amount must be a positive whole number of cents","request_id":"r1","retryable":false}
  [INFO ] request_id=r2 code=REFUND_PAYMENT_NOT_FOUND payment pay_9 not found
r2: 404 {"code":"REFUND_PAYMENT_NOT_FOUND","message":"payment pay_9 not found","request_id":"r2","retryable":false}
  [INFO ] request_id=r3 code=REFUND_EXCEEDS_CAPTURED refund of 9000 exceeds the refundable 5000
r3: 409 {"code":"REFUND_EXCEEDS_CAPTURED","message":"refund of 9000 exceeds the refundable 5000","request_id":"r3","retryable":false}
r4: 200 {"refund_id":"rf_1","request_id":"r4"}
r5: 200 {"refund_id":"rf_1","request_id":"r5"}
  [INFO ] request_id=r6 code=REFUND_IDEMPOTENCY_CONFLICT idempotency key reused with a different request
r6: 409 {"code":"REFUND_IDEMPOTENCY_CONFLICT","message":"idempotency key reused with a different request","request_id":"r6","retryable":false}
processor calls=3 refundable left=3000 outbox=["refund rf_1 payment=pay_1 amount=2000"]
```

(Request r4 met two `Unavailable` failures before succeeding, hence three processor calls; r5 is the client retrying r4
with the same key and gets the same refund without calling the processor again.) Write your review first, then compare
with the redesign and the model answers. The redesign is one defensible answer, not the only one.

---

## Interview mode

*Senior-level. Answer aloud or in writing, without notes, before checking Appendix A.*

### Language

1. Compare Java checked exceptions, Go error returns, and Rust's `Result`. What does each make visible, what does each
   let you ignore, and how does each compose with lambdas/closures?
2. What does `?` desugar to? Where does error conversion happen, and why is `?` on an `Option` rejected in a function
   returning `Result`?
3. When do you return `Option`, `Result`, `Result<Option<T>, E>`, or panic? Give a production example of each.

### Compiler and runtime

4. What does rustc emit for `?` on the happy path in release mode? What does a panic's landing pad look like, and why is
   unwinding "zero-cost" until it happens?
5. List the ways a Rust program can abort instead of unwinding. For each, say why continuing would be unsafe or
   impossible.
6. What exit statuses does the OS see for: `main` returning `Err`, an unwinding panic out of `main`, `process::abort`?

### Performance

7. Why can a large error type slow down the *success* path? What does the assembly show, and what are the fixes?
8. What do `Box<dyn Error>` and `anyhow::Error` cost compared with an enum? When does that matter?
9. A service uses panics plus `catch_unwind` for input validation. What would you measure, and what do you expect?

### Architecture

10. Library vs application errors: what's the real dividing line, and how do `thiserror` and `anyhow` fit?
11. Design the panic strategy and panic boundaries for a Tokio gateway, a JVM-hosted native library, and a CLI.
12. `Drop` can't fail. How do you design a transaction or file API whose close/commit can fail, and what should the
    error say when the outcome is unknown?

### Distributed systems

13. Why is a timeout ambiguous? How do idempotency keys make retries safe, and what are the three most common ways to
    get them wrong?
14. How do you prevent retry amplification across three layers of services?
15. What goes into the client response, the log line, and the metric for an internal error? What is the cardinality
    rule for error metrics, and why?

---

## Looking ahead: Part IX

Part VIII's measurements kept coming back to memory: the size of a `Result` paid on every return, an allocation per
boxed error, a 528-byte stack frame for one large error. Part IX turns that lens on the data structures every program
is built from: `Vec`, `String`, `HashMap`, `VecDeque`, `BTreeMap`, slices, and arrays. For each one it covers the
memory layout, the growth strategy, allocation behavior, and cache locality, measured the same way. It also delivers the
Interlude promised in Chapter 2.4: BFS versus DFS, down to the stack page.
