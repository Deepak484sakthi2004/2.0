# Part V Review — Type-Driven Design Capstone & Interview Mode

> Consolidate Part V, then use it: redesign a real workflow so its invalid states and invalid transitions don't
> compile, within a memory budget, with honest boundaries. Then answer senior-level questions without notes.
> Answers are in **Appendix A, Part V**.

---

## Part V on one page

```text
 COUNT            |struct| = product, |enum| = sum, Option = 1 + T, Result = T + E, Infallible = 0
                  RULE: values the type admits == states the domain has; every extra value is defended everywhere
        │
 PARSE AT EDGES   private field + parse(raw) -> Result<Type, Error>; core takes Type, never raw
                  sentinels (0 = unlimited) are enum variants not yet written;  .ok() erases "malformed"
        │
 LAYOUT           size % align == 0; default repr reorders (Order 24 vs repr(C) 32); repr(C) marks a boundary
                  niches: invalid bit patterns hold tags; count them (Option<Option<NonZeroU64>> = 16, Msg2 = 16)
                  guaranteed: Option<&T/Box/NonNull/NonZero/fn> only; the rest is [RUSTC]
                  padding is uninitialized: never bytes-on-disk (bytemuck refuses it); packed fields: no references
        │
 ZERO-COST TYPES  newtype = new rules, same bytes (discount_typed = discount_raw, an ALIAS in the asm)
                  ZST = a fact with no bytes (Vec<ZST>: 0 allocations); HashSet<K> = HashMap<K, ()>
                  PhantomData<fn() -> T> for IDs (covariant, Send + Sync); PhantomData<T> leaks !Send; invariant = brand
                  derive(Clone, Copy, Debug...) adds T: Trait bounds: implement by hand for phantom types
                  marker traits = claims (Idempotent), on_unimplemented = the policy's error message
        │
 TYPE-STATE       state = type, transition = fn(self) -> Next; wrong state → E0599; stale handle → E0382
                  sealed State trait closes the set; transitions cost nothing (settle_typed = one mov)
                  run-time data (rows, webhooks) → AnyX enum at the boundary delegating to typed transitions
                  types prove in-process control flow; the database and idempotency keys handle the rest
```

## Ten ideas to carry forward

1. **Count values.** If the type admits more values than the domain has states, the difference is a bug waiting for a
   constructor.
2. **Parse once, at the boundary.** Validation that returns `bool` has to be repeated. Parsing that returns a type
   doesn't.
3. **Privacy makes proofs unforgeable.** A newtype with a `pub` field is a label, not a guarantee.
4. **Sentinels are missing variants.** `0`, `-1`, and `""` meaning "not a value" belong in an enum, usually at zero
   cost thanks to niches.
5. **Exhaustiveness is change management.** Wildcards hand that job back to humans.
6. **Layout is unspecified until you pin it**, and `repr(C)` should always point at the boundary that needs it.
7. **Niches are counted, not assumed.** Only a few are guaranteed, and nested `Option`s use them up.
8. **Newtypes, ZSTs, and `PhantomData` are free.** The assembly proves it. Choose `PhantomData` markers for their
   variance and auto-trait effects.
9. **Type-state needs moves.** State-specific methods reject the wrong transition, and consuming `self` rejects the
   stale one.
10. **Types prove what one program does.** Distributed correctness still needs the database, idempotency, and
    reconciliation. Say so when you present the design.

---

## Capstone: a type-driven refund workflow

Meridian's refund service (Java) is being rewritten in Rust as part of the payments core from Chapter 5.4. This is the
current model:

```java
public class Refund {
    public long id;
    public long paymentId;
    public long tenantId;
    public long amount;          // minor units... or percent of the capture, for "partial" refunds (both have happened)
    public String currency;      // "EUR", "USD", sometimes "eur"
    public String status;        // "REQUESTED", "APPROVED", "REJECTED", "SENT", "SETTLED"
    public long requestedBy;
    public Long approvedBy;      // set when APPROVED; null otherwise
    public String rejectReason;  // set when REJECTED; free text
    public String pspRef;        // set when SENT
    public boolean partial;
    public Instant settledAt;    // set when SETTLED
}
```

**Business rules:**

- A refund is **requested** for a captured payment, by a user. Its amount must be positive and at most the captured
  amount, in the capture's currency.
- It must be **approved** by a *different* user (four-eyes rule) or **rejected** with one of a fixed set of reasons.
- An approved refund is **sent** to the PSP, which returns a reference.
- The PSP later calls a webhook: **settled** (with the reference and a timestamp). A webhook for an unknown reference or
  a refund in the wrong state must be rejected and counted, not applied.
- Refunds live in PostgreSQL, and there are about 200 million historical rows. The service keeps active refunds (about
  2 million) in memory.

**Your task:**

1. **Count.** Estimate how many combinations of `status`, the nullable fields, and `partial` the Java class admits, and
   how many are legal. List five specific illegal combinations and the bug each one invites.
2. **Types.** Design the Rust model:
   - newtypes and typed IDs (which fields become which types, and which constructors are private);
   - the amount (how "positive" and "at most the capture" are enforced, and where);
   - the states (markers or data-carrying?) and the transitions, including the four-eyes rule. What does a failed
     approval return?
3. **Boundaries.** Write the signatures for loading a row, applying a webhook, and persisting a transition. Where do
   run-time rejections live? How does an unknown `status` string behave?
4. **Layout budget.** Estimate the in-memory size of one active refund in your design, and of 2 million. Which fields
   have niches? Would the run-time enum over all states need a separate tag? Verify with `size_of`.
5. **What types can't do.** The PSP call can time out, and the process can crash between the PSP response and the
   database write. List what handles each case. None of the answers are types.

A model answer (listing `review-refund-model.rs`, verified) prints:

```text
request 0: Err(NotPositive)
request 6000: Err(ExceedsCapture { requested: 6000, captured: 4999 })
approve by requester: SelfApproval
approved by UserEntity#9
webhook rejected: psp ref mismatch for RefundEntity#2; still SENT
RefundEntity#2 of PaymentEntity#42: 1200 Eur settled at 1760000000 (psp psp-77)
RefundEntity#3 rejected: DuplicateRequest
webhook rejected: settlement webhook for a refund in state REJECTED
size_of: RefundId=8 Option<RefundId>=8 Refund<Requested>=40 Refund<Settled>=64 AnyRefund=64
```

Write your design first, then compare with it and with the discussion in Appendix A. There are several defensible
designs, and the key explains the trade-offs the model answer made.

---

## Interview mode

*Senior-level. Answer aloud or in writing, without notes, before checking Appendix A.*

### Modeling

1. What does "make invalid states unrepresentable" mean? Show it with a counting argument, and name one case where you'd
   deliberately *not* do it.
2. Explain "parse, don't validate" to a Java team. What changes in the codebase's structure, not only in the code?
3. How does exhaustiveness checking help with change management, and when would you give it up with
   `#[non_exhaustive]`?

### Layout

4. How does rustc lay out a struct by default, and when do you need `repr(C)`? What goes wrong if you use `repr(C)`
   everywhere?
5. What's a niche? Predict the size of `Option<Option<&u8>>` and explain it.
6. Which `Option` layout optimizations can FFI code rely on, and why only those?
7. Why is writing a struct's bytes to disk dangerous, even for a `repr(C)` struct of integers?

### Zero-cost types

8. Prove to a skeptic that a newtype costs nothing. What evidence would you show?
9. Design a typed ID. Which `PhantomData` marker do you choose, and what do the alternatives break?
10. Why can `#[derive(Clone, Copy)]` on a phantom-typed struct fail to make it `Copy`?

### Type-state

11. Implement a connection with `Disconnected`, `Connected`, and `Authenticated` states. Where do the socket and session
    live, and what does a failed `authenticate` return?
12. Why does type-state depend on move semantics? What does Java's version of the pattern miss?
13. How do you persist and reload a type-state entity without duplicating its transition logic?

### Architecture

14. Where would you use type-state in a payments system, and where would you refuse to?
15. A colleague claims type-state "solves double charging." Give the precise version of what it solves and what it
    doesn't.

---

## Looking ahead: Part VI

Part V kept running into traits without explaining them: `From` and `TryFrom` at boundaries, derived traits with
surprising bounds, marker traits with no methods, a sealed trait closing a set of states, and auto traits steered by
`PhantomData`. Part VI opens the trait system: bounds and default methods, associated types and blanket impls,
coherence and the orphan rule (the reason the sealed-trait trick works), trait objects and vtables, and the architect's
decision between static and dynamic dispatch. Among other things, it's where `AnyPayment`-style enums meet their main
alternative, `Box<dyn Trait>`.
