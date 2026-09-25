# Part VII Review — Instance Accounting & Interview Mode

> Consolidate Part VII, then use it: review a generic API the way a principal engineer would, by counting what it
> compiles to and deciding what deserves to stay generic. Then answer senior-level questions without notes. Answers are
> in **Appendix A, Part VII**.

---

## Part VII on one page

```text
 SOURCE            fn f<T: Bound>(x: T)  — one definition; T may use ONLY what Bound promises
                   author's mistake → E0369 at the body;  caller's mistake → E0277 at the call
                   exception: const evaluation per instance (E0080 "while instantiating") — cargo check can miss it
        │
 COLLECTOR         walks from roots (main / pub non-generic items); records f::<u8>, f::<String>, ...
                   unused generics → no code; lifetimes erased → never instantiated
                   instances are compiled in the crate that needs them (the recipe ships as MIR)
        │
 MACHINE CODE      one symbol per instance, v0 names carry type args (_R...7largesthE → largest::<u8>)
                   each instance optimized for its T: cmova (u8), cmovg (i64), maxsd blend (f64);
                   u32 sum vectorized, f64 sum sequential (FP isn't associative)
                   static calls → inlining: iterator chain = 22 debug functions → 2 release functions
        │
 VS JAVA           erasure: one body, T → Object/bound, checkcast at call sites, bridge methods, boxing
                   (1 alloc vs 1,000,001 measured), no new T()/T[]/T.class; JIT recovers via type profiles,
                   stopped by profile pollution and warm-up; C# shares ref types, Go shares GC shapes
        │
 COST              cost ≈ body (incl. everything it instantiates) × distinct instantiations — exactly linear
                   closures in generic fns are generic too → they multiply
                   levers: thin shell + non-generic inner (88 → 26 IR fns), concrete types, dyn at cold
                   boundaries, fewer type params, cargo check, crate splits (help less than you'd think)
        │
 impl Trait        argument position = anonymous type param (can't be turbofished: E0107, a semver hazard)
                   return position = one hidden concrete type, static dispatch
                   edition 2024: RPIT captures all in-scope lifetimes (E0502 surprise); use<..> says less
```

## Ten ideas to carry forward

1. **Checked once, stamped per type.** A generic body is type-checked against its bounds, then copied for each concrete
   type that is actually used.
2. **The bound is the contract**, for both sides: the author may use only what it promises, and every type that
   satisfies it gets an instance. Review trait impls like functions.
3. **Instances are real functions**, with real symbols, and profilers and crash reports show their type arguments.
4. **Monomorphization makes calls static, and inlining makes them disappear.** Neither step alone gives zero-cost
   iterators.
5. **Per-type code means per-type semantics**, such as vectorized integer sums and strictly ordered floating-point sums.
6. **Erasure trades run-time speed for compatibility and dynamism.** Rust made the opposite trade, and `dyn` is where
   you buy some dynamism back, explicitly.
7. **`dyn Any` is erasure you chose.** Justify it like `unsafe`.
8. **Cost is body × instantiations, and the body includes everything it instantiates.** Closures multiply with their
   enclosing function.
9. **Keep the generic part thin.** Generic where the type changes the machine code on a hot path, concrete or `dyn`
   everywhere else.
10. **Signatures are contracts beyond types.** Explicit type parameters, `impl Trait` arguments, and `use<..>` captures
    are all part of the public API.

---

## Capstone: instance accounting for Meridian's event publisher

Meridian's platform team wrote a Rust event publisher for all services. Every collaborator became a type parameter,
"for flexibility and testability". The version under review:

```rust,ignore
// BEFORE (sketch, not compiled): every collaborator is a type parameter.
pub struct Publisher<T: Transport, C: Codec, R: RetryPolicy, M: Metrics> {
    transport: T,
    codec: C,
    retry: R,
    metrics: M,
}

impl<T: Transport, C: Codec, R: RetryPolicy, M: Metrics> Publisher<T, C, R, M> {
    /// ~60 lines: encode with the codec, retry loop with backoff and jitter, metrics on every
    /// attempt, error mapping with context strings built in closures.
    pub fn publish<V: Serialize>(&mut self, topic: impl AsRef<str>, event: &V) -> Result<(), PublishError> {
        /* ... */
    }
    // ... 11 more methods (flush, close, health, publish_batch, ...), all using the same four parameters
}
```

What is available per service: transports `Kafka`, `FileSpool`, and `Memory` (tests); codecs `Json` and `Proto`; retry
policies `Fixed`, `Exponential`, and `NoRetry`; metrics `Prometheus` and `Noop`. A typical service publishes **40**
event types, and passes topics as both `&str` and `String`. Production uses one combination: Kafka, Proto,
Exponential, and Prometheus. Tests use Memory, Json, NoRetry, and Noop.

**Your tasks:**

1. **Count.** How many instances of `publish`'s 60-line body does a typical service's release binary contain? How many
   does its test build add? What else is instantiated per instance (think about the closures)?
2. **Classify each parameter.** For each of `T`, `C`, `R`, `M`, `V`, and the topic parameter, ask: does the type change
   the machine code on the hot path, and does production use more than one type at a time? Decide: keep generic, make
   `dyn`, make a value (enum or struct), or make concrete.
3. **Redesign** the publisher. Keep one generic method at most, and keep it thin.
4. **State what you gave up**, and how you'd prove it doesn't matter (name the measurement).
5. **Write the review comment** you'd leave on the original PR, in five sentences or fewer.

**One acceptable redesign, verified.** The generic surface shrinks to a thin `publish<E: Encode + ?Sized>` shell. The
sending, retrying, and metrics code is compiled once, and the transport sits behind `Box<dyn Transport>` (verified,
`listings/part-07/review-generic-api.rs`):

```rust,ignore
//! Meridian's event publisher after the Part VII review:
//! generic at the edge where types differ, `dyn` and plain functions where they don't.

/// How an event turns itself into bytes. Implemented per event type (hot, inlinable).
pub trait Encode {
    fn encode(&self, out: &mut Vec<u8>);
}

/// Where bytes go. Implemented by the Kafka client, a file sink, or a test double.
pub trait Transport {
    fn send(&mut self, topic: &str, bytes: &[u8]) -> Result<(), String>;
}

pub struct Publisher {
    transport: Box<dyn Transport>, // one copy of all sending code, whatever the transport is
    buf: Vec<u8>,                  // reused across publishes: no allocation per event once warm
    pub sent: u64,
    pub failed: u64,
}

impl Publisher {
    pub fn new(transport: impl Transport + 'static) -> Self {
        Publisher { transport: Box::new(transport), buf: Vec::with_capacity(512), sent: 0, failed: 0 }
    }

    /// The only generic method: a thin shim that encodes, then hands off to shared code.
    pub fn publish<E: Encode + ?Sized>(&mut self, topic: &str, event: &E) -> Result<(), String> {
        self.buf.clear();
        event.encode(&mut self.buf);
        self.send_encoded(topic)
    }

    /// Non-generic: retries, metrics, and error mapping are compiled once.
    fn send_encoded(&mut self, topic: &str) -> Result<(), String> {
        let mut last = String::new();
        for _attempt in 0..3 {
            match self.transport.send(topic, &self.buf) {
                Ok(()) => {
                    self.sent += 1;
                    return Ok(());
                }
                Err(e) => last = e,
            }
        }
        self.failed += 1;
        Err(format!("{topic}: gave up after 3 attempts: {last}"))
    }
}

pub struct PaymentCaptured {
    pub payment_id: u64,
    pub amount_cents: i64,
}
impl Encode for PaymentCaptured {
    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.payment_id.to_le_bytes());
        out.extend_from_slice(&self.amount_cents.to_le_bytes());
    }
}
impl Encode for str {
    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Test double: records what was sent; fails the first `flaky` sends.
    struct Memory {
        log: Rc<RefCell<Vec<(String, Vec<u8>)>>>,
        flaky: u32,
    }
    impl Transport for Memory {
        fn send(&mut self, topic: &str, bytes: &[u8]) -> Result<(), String> {
            if self.flaky > 0 {
                self.flaky -= 1;
                return Err("broker unavailable".into());
            }
            self.log.borrow_mut().push((topic.to_string(), bytes.to_vec()));
            Ok(())
        }
    }

    #[test]
    fn publishes_two_event_types_through_one_transport() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut p = Publisher::new(Memory { log: Rc::clone(&log), flaky: 0 });
        p.publish("payments", &PaymentCaptured { payment_id: 7, amount_cents: 1250 }).unwrap();
        p.publish("audit", "captured 7").unwrap();
        assert_eq!(p.sent, 2);
        assert_eq!(log.borrow()[0].1.len(), 16);
        assert_eq!(log.borrow()[1].1, b"captured 7");
    }

    #[test]
    fn retries_then_gives_up() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut p = Publisher::new(Memory { log: Rc::clone(&log), flaky: 2 });
        assert!(p.publish("audit", "ok on third try").is_ok());
        let mut q = Publisher::new(Memory { log, flaky: 5 });
        let err = q.publish("audit", "never").unwrap_err();
        assert!(err.contains("gave up after 3 attempts"), "{err}");
        assert_eq!((q.sent, q.failed), (0, 1));
    }
}
```

```text
running 2 tests
test tests::retries_then_gives_up ... ok
test tests::publishes_two_event_types_through_one_transport ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

The tests still substitute a fake transport. Testability never required a type parameter, only a trait. `?Sized` lets
`str` be an event (`publish("audit", "captured 7")`), and taking `&str` for the topic removed a generic parameter that
bought nothing. Retry policy and metrics are omitted here for brevity. In the full design they are values held by the
publisher, an enum for the policy and an `Arc<dyn Metrics>` for metrics, configured at startup. Compare your answers
with Appendix A.

---

## Interview mode

*Senior-level. Answer aloud or in writing, without notes, before checking Appendix A.*

### Language

1. What does a trait bound on a generic parameter allow the body to do, and what does it require of callers? Where is
   each violation reported?
2. What is the difference between `fn f<T: Display>(x: T)`, `fn f(x: impl Display)`, and `fn f(x: &dyn Display)`,
   for callers, for the binary, and for semver?
3. What does return-position `impl Trait` promise and hide? What changed in edition 2024, and what is `use<..>` for?

### Compiler

4. Walk through monomorphization: the collector, its roots, mono items, codegen units, and symbols. Where are instances
   of a dependency's generic functions compiled?
5. Which checks happen per instantiation rather than per definition, and why does that matter for `cargo check`?
6. Why do closures inside a generic function multiply with it? How do you stop that?

### Java comparison

7. Explain erasure, `checkcast`, and bridge methods to a Rust engineer who has never used Java. Then explain
   monomorphization to a Java engineer, in terms of what the JIT does.
8. What is profile pollution, and why can't it happen to a Rust instance?
9. Place Java, C#, Go, C++, and Rust on a map of "where is generic code shared, and where is it specialized".

### Performance

10. The same generic `sum` vectorizes for `u32` and not for `f64`. Explain, and say what you'd do if the order of a
    floating-point sum didn't matter.
11. Quantify the cost of boxing a million `i64`s, in allocations and bytes, and explain the CPU effect on iteration.

### Architecture

12. How do you decide, per parameter, between generic, `dyn`, enum, and concrete types in a library API used by 30
    services?
13. How would you keep a large workspace's build time under control as it grows? What do you measure, and what do you
    enforce in CI?
14. A team proposes `HashMap<String, Box<dyn Any>>` for configuration "like we had in Java". What's your review comment?

---

## Looking ahead: Part VIII

Part VII's generics carried a quiet assumption: every function succeeded. Part VIII, *Error Handling*, puts failure
back. It covers `Result` and `Option` as ordinary generic enums (every `Result<T, E>` is monomorphized like any other
generic type), the `?` operator and the `From` conversions it relies on, how to design error types for libraries versus
applications, panics and unwinding at the machine level (the unwind paths Chapter 3.5 showed in MIR), and errors at
service boundaries. The `String` errors of Projects L1 and L2 finally get proper types.
