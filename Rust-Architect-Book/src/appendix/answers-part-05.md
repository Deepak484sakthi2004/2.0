# Appendix A — Answer Key: Part V

> Model answers. Write yours first. Where several answers are defensible, the key says so. Sizes quoted here were
> verified on rustc 1.98.1 (listings named inline; `review-answer-sizes.rs` collects the ones not shown in chapters).

---

## Chapter 5.1 — Algebraic Data Types

### Interview & architecture questions

**1. Products and sums.** A struct holds all its fields at once, so its values are every combination:
`|A| × |B|`. An enum holds exactly one of its variants, so its values are the union: `|A| + |B|`.
`(bool, Option<bool>)` has 2 × (1 + 2) = **6** values, and `Result<bool, ()>` has 2 + 1 = **3**.

**2. Values = states.** A type should admit exactly as many values as the domain has states. `ConnFlags { connected:
bool, authenticated: bool }` admits 4 values for 3 states. The fourth ("authenticated, not connected") is eventually
produced by a partial update or a bad row, and then each function handles it differently: one sends a query on a
closed socket, another reconnects without re-authenticating. Every extra value is a case each function must defend
against, or silently assume away.

**3. Validate vs parse.** Validation checks a value and returns `bool` or `()`, so the knowledge is thrown away, and
the next function must re-check or trust. Parsing returns a *more precise type* that carries the proof. In a small
codebase you can remember where validation happens. In a large one, the number of places that must remember grows with
the code, and one missing call (Meridian's CSV importer, 1 of 14) is enough. Parsing makes the rule structural: core
functions *can't be called* with unparsed data.

**4. Unforgeable `Email`.** The field is private, so outside the module the tuple-struct constructor is private too
(E0603, verified), and the only public constructor is `parse`. Ways to weaken it: a `pub` field; a public
`From<String>` or `new` that skips the checks; a `Deserialize` derive that builds the struct directly (fix:
`#[serde(try_from = "String")]`, Chapter 5.3 §9); a `Default` impl producing an invalid value; a method handing out
`&mut String`; or `unsafe` transmutes.

**5. Exhaustiveness.** rustc's `rustc_pattern_analysis` runs Maranget's usefulness algorithm on THIR. Treating the arms
as rows of a pattern matrix, it asks whether a wildcard would still be *useful* after the last arm, meaning whether some
value is matched by no arm. If so, it reports that value as E0004 ("`&LedgerEvent::Chargeback { .. }` not covered").
Guards are arbitrary boolean expressions whose truth can't be decided at compile time, so a guarded arm is assumed to
possibly *not* match and covers nothing for exhaustiveness purposes.

**6. `otherwise: unreachable`.** It tells LLVM that the discriminant can only take the listed values, so no default
path is needed (and a jump table needs no bounds check). It's true because of the **validity invariant**: an enum value
with any other tag is undefined behavior to create (Chapters 1.3 and 2.3; Miri-verified in Chapter 5.2 §4).

**7. Sizes from counting.** `Result<u64, Infallible>` has 2⁶⁴ + 0 values, which fit exactly in 64 bits: 8 bytes, no tag.
`Option<u64>` has 2⁶⁴ + 1 values, one more than 64 bits can hold, and `u64` has no invalid bit patterns to borrow, so it
needs a separate tag byte, padded to the 8-byte alignment: 16. In general, a niche is possible only if
`1 + |T| ≤ 2^(8 × size_of::<T>())`, meaning the payload has spare bit patterns.

**8. `#[non_exhaustive]`.** Use it on *public* enums in libraries that are likely to grow: error enums, protocol
message kinds, configuration options. Downstream crates must add a wildcard arm, so adding a variant is no longer a
breaking change. The cost falls on users: they lose the compile error that would point at every place a new variant
matters. Don't use it on your own application's domain enums, where exhaustiveness is exactly what you want.

### Debugging exercise (`ok-swallow`)

1. `"30s"` fails to parse as `u32`, `.ok()` turns the error into `None`, and `None` already meant "never time out." So
   idle connections are never closed, and the pool eventually exhausts the database's connection limit, hours later,
   far from the typo. Refusing to start would have failed in seconds, during the deploy, with the key named.
2. `.and_then(|v| v.parse().ok())`. Before it, the value is `Option<Result<u32, ParseIntError>>`: absent (1), present
   and valid (2³²), or present and malformed (|errors|). After it, `Option<u32>`: 1 + 2³². Every malformed value
   collapses into the value that means "absent."
3. Return `Result` and keep the distinction with `transpose`, as `ch01-04-combinators.rs` does:
   `raw.get("idle_timeout_s").map(|v| v.parse::<u32>()).transpose().map_err(|e| ConfigError::NotANumber("idle_timeout_s", e))?`.
   The key stays optional, and malformed values become errors.
4. A review rule: in parsing and config code, `.ok()` on a `Result` needs a comment explaining why an error is
   equivalent to absence. Otherwise use `?`, `map_err`, or `transpose`. At startup, malformed configuration is fatal.

### Selected exercises

- **Beginner:** `enum Shipment { Pending, Shipped { tracking: String }, Delivered { tracking: String } }`. Before:
  2 × 2 × 2 = 8 shapes (tracking present or absent), of which 3 are legal. After: exactly 3.
- **Advanced:** four arms: `(Some(a), Some(b))`, `(Some(a), None)`, `(None, Some(b))`, `(None, None)` (or three, with
  `(None, _)`). Add a guard to one arm and the compiler reports E0004 with the pattern that arm used to cover, because a
  guarded arm covers nothing.
- **Systems:** the MIR reads `discriminant(_1)` and `switchInt`s on it, like Chapter 5.1 §4. In release assembly, the
  discriminant of a niche-encoded `Option<&u64>` becomes `test rdi, rdi`: the `None` check *is* the null check
  (`deref_or_max` in `ch02-03-niche-asm.rs`).

---

## Chapter 5.2 — Type Layout

### Interview & architecture questions

**1. Layout facts.** Alignment (a power of two), size, and validity (the set of legal bit patterns). Size is a multiple
of alignment, and each field's offset is a multiple of that field's alignment. Consequently, arrays have no gaps
between elements.

**2. Reordering.** To minimize padding, and to leave room for tag and niche choices (verified: `Order` is 24 bytes by
default and 32 as `repr(C)`). Prevent it with `repr(C)` when layout crosses a boundary: FFI, memory-mapped or on-disk
structures, raw wire structs, and `unsafe` code that computes offsets or casts pointers between types.

**3. Nested niches.** `NonZeroU64` has exactly one invalid pattern (0). `Option<NonZeroU64>` uses it for `None`, so an
outer `Option` needs a second spare pattern, and none is left. That forces a tag: 8 + 8 = 16. `bool` has 254 invalid
patterns (2–255). Each `Option` layer consumes one (2, 3, 4), so three layers still fit in one byte.

**4. `Msg2` vs `Payload`.** In `Msg2`, the `Box` has one niche (null), and there's exactly one other variant to encode.
`Small`'s `u32` can be stored in the bytes of the `u64` field, which don't overlap the `Box`, so "the `Box` is null"
means "this is `Small`": 16 bytes, the size of `Big`. In `Payload`, the `Big` variant *is* the `Box`, all 8 bytes of
it, so `Small`'s `u8` has nowhere to live except on top of the niche field. A tag is needed: 16.

**5. Guaranteed niches.** `Option` of `&T`, `&mut T`, `Box<T>`, `NonNull<T>`, `NonZero*` integers, and function
pointers (and `repr(transparent)` wrappers of these) has the inner type's size, alignment, and ABI, with `None` as the
all-zero pattern (std docs, "Representation"). FFI and `transmute` need contracts that won't change between compiler
versions. Everything else (`Option<bool>`, `Option<char>`, multi-variant niches, field order) is current rustc
behavior.

**6. Transmuting 7 into `Side`.** Producing a value that violates its type's validity invariant is undefined behavior
*at the moment it's produced* (Miri reports it at the transmute, verified). The compiler assumes every `Side` is 1 or 2
and builds on that assumption elsewhere: it stores `None::<Side>` as 0, it emits `unreachable` for other tags in
`match`, and LLVM gets range metadata on loads. A 7 could be misread as another enum's niche value or send a `match` off
into unreachable code.

**7. E0793.** A reference promises alignment (rustc gives LLVM an `align` attribute), and creating a misaligned
reference is UB even if it's never dereferenced. A packed field can sit at any offset. The alternatives: copy the field
out by value (`let len = h.length;`, which compiles to an unaligned load), or take a raw pointer (`addr_of!` or
`&raw const`) and use `read_unaligned` / `write_unaligned` (verified clean under Miri).

**8. False sharing.** Two independently written variables share one cache line. Every write by one core invalidates the
other core's copy, so the line bounces between cores even though no data is shared. `#[repr(align(64))]` gives each
value its own line (verified: counters on lines 0–7 instead of all on line 0). The cost is memory and cache footprint:
eight 8-byte counters grow from 64 to 512 bytes, and reading all of them now touches 8 lines instead of 1.

### Debugging exercise (negative cache)

1. Counting: there are 2⁶⁴ − 1 account IDs plus two extra states ("unknown", "absent"). `NonZeroU64` has one spare
   pattern, and two are needed, so a tag is required: 16 bytes. The explicit enum `Cached` has the same count (verified
   16), so naming the states doesn't change the arithmetic.
2. "Unknown" is already represented by the key being **absent from the map**. Store `Option<NonZeroU64>`: a present key
   with `None` means "known to have no account." The value is 8 bytes (verified).
3. The entry shrinks from 24 to 16 bytes (verified), saving 8 × 50M = **400 MB** of entry payload. The hash table's own
   overhead (control bytes and load-factor slack) is on top of that. Expiry is the next state, and adding a `u32`
   timestamp would make the entry 24 bytes again. Two ways to keep 16 bytes: **generational expiry** (two maps,
   current and previous; every N minutes drop the previous and demote the current, so the expiry granularity is the
   epoch and no per-entry timestamp is needed), or, if the ID space allows, `NonZeroU32` IDs plus a `u32` expiry in the
   same 8 bytes.

### Selected exercises

All sizes below were verified in `review-answer-sizes.rs`:

- **Beginner:** `struct A { a: u8, b: u16, c: u8 }` is **4** bytes by default (`b` at 0, `a` at 2, `c` at 3) and
  **6** as `repr(C)` (`a` at 0, `b` at 2, `c` at 4, then 1 byte of padding).
- **Intermediate:** in declaration order, the `repr(C)` struct is **32** bytes. Ordered largest first (`ts: u64`,
  `qty: u32`, `code: u16`, `flag: bool`, `kind: u8`), it's **16**, the same as the default repr.
- **Advanced:** `Option<(NonZeroU32, bool)>` = **8**: the niche comes from `bool` (254 spare values), not from
  `NonZeroU32`. `Result<&u8, u8>` = **16**: `Err`'s byte can't live outside the pointer's bytes. `Option<Result<(),
  bool>>` = **1**: `Result<(), bool>` uses 2 for `Ok(())`, and `Option` uses 3 for `None`. `enum E { A(char), B, C, D }`
  = **4**: `char`'s invalid values encode `B`, `C`, and `D`.
- **Systems:** expect the niche version to be close to 2× faster on slices far larger than the cache, because of bytes
  moved alone, and the gap to shrink for cache-resident slices. Report medians and the machine. The book doesn't quote a
  number for you.

---

## Chapter 5.3 — Newtypes, ZSTs, PhantomData, and Markers

### Interview & architecture questions

**1. Newtype.** A new *nominal* type wrapping an existing one. The type checker distinguishes it by name, and the
distinction is erased before code generation. The evidence: in release assembly, `discount_typed` was emitted as an
**alias** of `discount_raw` (identical machine code, merged), and `shard_typed` and `shard_raw` compiled to the same
instructions. Also `size_of::<BasisPoints>() == 2`.

**2. Private vs public field.** Make the field private when the type carries an invariant: `BasisPoints` must be at
most 10,000, and `Email` must have been parsed. A public field is fine when every inner value is valid and the type
only separates *meaning*: `Cents(pub i64)` keeps cents from being confused with basis points, and any `i64` is a
legitimate amount.

**3. `PhantomData`.** It tells the compiler the struct's variance in the parameter, its auto-trait behavior (`Send`,
`Sync`, and so on), and whether dropping it may drop a `T` (drop check). An unused parameter is rejected (E0392) because
the compiler couldn't answer those questions, and the parameter would be meaningless.

**4. Typed-ID marker.** `PhantomData<fn() -> T>` is covariant, is always `Send + Sync`, and doesn't claim ownership. An
ID *refers to* an entity, it doesn't contain one. With `PhantomData<T>`, `Id<Session>` becomes `!Send` when `Session`
holds an `Rc` (verified E0277), and it claims ownership for drop check. With `PhantomData<*const T>`, every ID is
`!Send` and `!Sync`.

**5. ZSTs.** A type of size 0. Its values carry no data, only the fact that they exist. `Vec<()>::push` increments `len`
and never allocates: capacity is `usize::MAX`, and 1M pushes made 0 allocations (verified). `HashSet<K>` wraps
`HashMap<K, ()>`, and since `()` is 0 bytes, each entry is just the key (`(u64, ())` is 8 bytes).

**6. Marker vs auto trait.** A marker trait like `Idempotent` is implemented explicitly, and each `impl` is a claim that
code review must check. The compiler only checks that bounds are satisfied. Auto traits (`Send`, `Sync`, `Unpin`) are
implemented *by the compiler*, structurally from the fields. Manually implementing `Send`/`Sync` is `unsafe`, and that's
where the human claim lives.

**7. `repr(transparent)`.** It's required whenever anything relies on the newtype being layout- and ABI-identical to
its field: `extern "C"` signatures, pointer casts or transmutes between `&[u16]` and `&[BasisPoints]`, and the `Option`
niche guarantee through a wrapper. Elsewhere it's harmless. rustc usually passes single-field structs the same way
anyway, but only the attribute guarantees it.

**8. `Deref<Target = u64>`.** Auto-deref makes every `u64` method and operator reachable through the ID (`id.pow(2)`,
`*id + 1`, comparisons with raw numbers), which quietly undoes the newtype. `Deref` is meant for smart pointers (Rust API
Guidelines, C-DEREF). Expose `get()` or `as_u64()` explicitly instead.

### Debugging exercise (derived bounds)

1. `#[derive(Copy)]` generates `impl<T: Copy> Copy for Id<T> {}`, and `Clone` likewise generates `T: Clone`. `Order`
   owns a `Vec`, so it isn't `Copy`, and neither is `Id<Order>`. The second `audit(id)` is a use after move (E0382, with
   rustc's note "derived `Clone` adds implicit bounds on type parameters").
2. Every derive bounds every type parameter: `PartialEq` (`T: PartialEq`), `Eq`, `Hash`, and `Debug`. Next failures:
   using `Id<Order>` as a `HashMap` key (E0277: `Order: Hash`/`Eq` not satisfied) and `a == b` (E0369).
3. Write the impls by hand without bounds, as `ch03-03-typed-ids.rs` does. Derive macros work on *syntax* before type
   checking: they see that `T` appears in a field type, and they can't know that `PhantomData<fn() -> T>` needs nothing
   from `T`, so they conservatively bound every parameter.
4. `#[derive(Debug)]` on `Payment<S>` requires `S: Debug`, and uninhabited markers like `enum Pending {}` don't
   implement it. That's the E0277 in `ch04-11-derive-debug.rs`.

### Selected exercises

- **Intermediate:** implement `Add<Cents>`, `Sub<Cents>`, and `Mul<BasisPoints> for Cents` (with an explicit rounding
  rule, and `i128` intermediates as in `ch03-01`). Omitting `Mul<Cents>` makes `Cents * Cents` fail with E0369. A
  `compile_fail` doctest pins that down.
- **Advanced:** with `raw: NonZeroU64`, `Option<Id<T>>` is 8 bytes (verified as `Option<RefundId>=8` in
  `review-refund-model.rs`). `parse("0")` should return an error (`IdError::Zero`), not `None`. `0` is malformed input,
  not an absent ID.
- **Systems:** predicted from Chapter 5.3 §6: identical instruction sequences for `&[Cents]` and `&[i64]`, likely merged
  into an alias. Verify it. If they aren't merged, check whether the panic locations differ.

---

## Chapter 5.4 — The Type-State Pattern

### Interview & architecture questions

**1. Two features.** Impl blocks for specific instantiations (methods exist only in legal states) and move semantics
(transitions consume `self`). Without the first, every method exists in every state, and you're back to run-time checks.
Without the second (a `Copy`/`Clone` state value, or a language with only shared references), a stale handle to an old
state can run the same transition again: the double capture.

**2. E0599.** Method probing collects inherent impls whose self type unifies with `Payment<Pending>`, then trait
methods in scope. `impl Payment<Captured>` doesn't unify, so `refund` isn't a candidate, and resolution fails. The note
comes from a separate, diagnostic-only search over impls of the same type constructor that ignores the type arguments.

**3. Markers vs data.** Use markers when every state carries the same data: a payment's ID and amount. Use
data-carrying states when some data exists only in some states: a connection's socket (connected and later) and session
(authenticated), or a refund's approver (approved) and PSP reference (sent and later). Then the data can't be missing,
and there are no `Option`s to unwrap.

**4. Not `Copy`.** A `Copy` state value survives its own transition, so `auth.capture()` could run twice. It can come
back through a derive: someone adds `#[derive(Clone, Copy)]` to `Payment<S>` and, to satisfy the derived bounds, also to
the marker types. Now both captures compile. `Clone` alone is also dangerous: `auth.clone().capture()` followed by
`auth.capture()`. Values that represent a one-time resource shouldn't be `Clone`.

**5. Sealed trait.** `pub trait State: sealed::Sealed`, where `Sealed` is a public trait inside a private module.
Downstream code can't *name* `Sealed`, so it can't implement it, and therefore can't implement `State`. That closes the
set of states: nobody can invent `AlreadyCaptured` and write their own transitions (verified E0277, with rustc's
"sealed trait" note). As a bonus, you can add items to `State` later without a breaking change.

**6. Persisting and reloading.** Parse the row into an enum over the typed states (`AnyPayment::from_row`). An unknown
state string is a load error. Apply dynamic events by matching `(state, event)` and calling the typed transition in each
legal arm. The logic isn't duplicated, because each arm can only call methods that exist on that state's type: the
run-time table can't declare an illegal transition legal. Run-time rejection lives only at this boundary.

**7. Cost.** `settle_typed` (two transitions and a read) compiled to `mov rax, rsi; ret`. `capture_checked` is a
compare, a branch, and a store. The difference is a nanosecond or less, well predicted. The real value is that the
invalid path, with its error handling, logging, and tests, doesn't exist in the typed design.

**8. Limits.** Type-state can't know the remote outcome after a timeout, survive a crash between the PSP call and the
database write, or stop two processes from both transitioning the same payment. Those are handled by, respectively, an
idempotency key (the PSP deduplicates), a database compare-and-set plus a reconciliation job (or an outbox), and the
compare-and-set again.

### Debugging exercise (`derive(Debug)` on `Payment<S>`)

1. The derive generates `impl<S: Debug> Debug for Payment<S>`. The bound is syntactic. The derive doesn't know `S` is
   only a marker, or that no `Pending` value can exist.
2. A manual impl needs nothing from the markers, and it can print the state name. Unverified sketch, in the style of
   `ch04-03-payment-typestate.rs`:

   ```rust,ignore
   impl<S: State> fmt::Debug for Payment<S> {
       fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
           f.debug_struct("Payment")
               .field("id", &self.id)
               .field("cents", &self.cents)
               .field("state", &S::NAME)
               .finish()
       }
   }
   ```

   Deriving `Debug` on the markers works too, but it spreads requirements onto marker types, and the next derive
   (`Clone`) will ask for more.
3. `Clone`, `Copy`, and `PartialEq` come next. `Clone`/`Copy` reintroduce the double capture (question 4). Make "no
   `Clone` on type-state values" a review rule.

### Selected exercises

- **Intermediate (`Disputed`).** The typed core forces nothing, because new impl blocks are additive. `AnyPayment`
  forces updates in every exhaustive `match` (`state_column`, `id`: E0004), but `apply` has a fail-closed catch-all
  `(s, e)` arm, so it will *not* force you to add the dispute transitions. They're silently rejected until someone
  writes them. That's the trade-off of catch-alls from Chapter 5.1, and it needs tests.
- **Advanced (`Transaction<'conn, S>`).** A `Drop` impl can't be specialized to one state (E0366, "`Drop` impl
  specialization"). Put the resource in an inner guard, `Option<TxGuard>` whose `Drop` rolls back, and have `commit`
  and `rollback` `take()` it. That's Chapter 3.5's guard pattern inside a type-state wrapper.
- **Systems.** In debug builds the transitions are real calls that move 16 bytes through the stack: several
  instructions each. "Zero-cost" is a claim about optimized builds.

---

## Part V Review — Capstone (refund workflow)

**1. Counting.** Looking only at the *shape* (which nullable fields are set) and `status`: 5 statuses × 2⁴ nullable
fields × 2 for `partial` = 160 shapes, before counting bad strings (`"eur"`, a `status` of `"SETTELD"`). Legal shapes:
5 (one per status, with exactly the fields that status implies), and `partial` should be derived from `amount`, not
stored. So roughly 150 illegal shapes. Five examples and their bugs:

- `SETTLED` with `pspRef == null`: reconciliation with the PSP is impossible.
- `APPROVED` with `approvedBy == requestedBy`: the four-eyes control is bypassed (a *value* bug, not a shape bug).
- `REJECTED` with `pspRef != null`: money may have moved on a rejected refund.
- `amount` greater than the capture, or negative: an over-refund, or a charge disguised as a refund.
- `amount` holding a percentage for "partial" refunds: the Chapter 2.2 unit bug again.
- `currency` `"eur"` vs `"EUR"`, or different from the capture's: currency mismatch or FX loss.

**2. The model answer** (`review-refund-model.rs`):

- **IDs**: `Id<T>` with `NonZeroU64` and `PhantomData<fn() -> T>`. `RefundId`, `PaymentId`, and `UserId` are distinct
  types, and `from_raw` is `pub(crate)`. `Option<RefundId>` is 8 bytes.
- **Amount**: `RefundAmount(NonZeroU64)` (positive by construction), created only by `Refund::request`, which checks
  it against the `CapturedPayment` view: parse, don't validate. The currency is **taken from the capture**, not from
  input, so a mismatch is unrepresentable. `partial` isn't stored at all. It's `amount < captured`, computable when
  needed.
- **States** carry their data: `Requested { requested_by }`, `Approved { approved_by }`,
  `Rejected { reason: RejectReason }` (an enum, not free text), `Sent { psp_ref }`, and `Settled { psp_ref,
  settled_at }`.
- **Four-eyes**: `approve(self, approver) -> Result<Refund<Approved>, (Refund<Requested>, RefundError)>`, which returns
  the request on self-approval (verified: "approve by requester: SelfApproval").
- **A deliberate trade-off**: the model's states *drop* earlier data (`Approved` no longer holds `requested_by`). The
  audit trail belongs in an append-only event table, not the live state. Keeping the history in the state types is a
  defensible alternative. It costs memory and makes every later state larger.
- **Tenant**: derived from the payment. Storing it on the refund invites the refund and payment disagreeing. If you
  store it, make it a `TenantId` and check it against the payment in `request`.

**3. Boundaries.** `fn from_row(row: &RefundRow) -> Result<AnyRefund, LoadError>` (an unknown status is a `LoadError`
naming the row ID, counted and alerted, never defaulted); `fn on_settled_webhook(self, psp_ref: &str, at: u64) ->
Result<AnyRefund, (AnyRefund, String)>` (verified: a reference mismatch and a wrong-state webhook are both rejected, and
the refund is handed back unchanged); and persistence per transition,
`fn persist_sent(tx: &mut Tx, r: &Refund<Sent>) -> Result<(), Conflict>`, running
`UPDATE refunds SET state = 'SENT', psp_ref = $2 WHERE id = $1 AND state = 'APPROVED'`, where zero rows means someone
else moved the refund first.

**4. Layout budget.** Verified: `Refund<Requested>` is 40 bytes, and `Refund<Settled>` and `AnyRefund` are 64. The enum
needs **no extra tag**: a niche in its largest variant absorbs it. Which niche rustc picks (the `repr(u8)` `Currency`
has 254 unused byte values, and the `String` has spare capacity values) is [RUSTC] behavior. So 2M active refunds at
≤ 64 bytes is at most 128 MB, plus each `psp_ref` string's heap buffer (an allocation per sent refund). Two ways to
shrink it: store PSP references as fixed-size byte arrays or interned IDs, and note that settled refunds aren't
"active", so most in-memory refunds are in the smaller early states. `Option<RefundId>` is 8 bytes, thanks to
`NonZeroU64`.

**5. What types can't do.** A PSP **timeout**: send the refund ID as an idempotency key, and query the PSP's status
endpoint before retrying. A **crash** between the PSP response and the database write: write a `SENDING` intent before
the call (an outbox), and run a reconciliation job that resolves intents older than N minutes against the PSP.
**Concurrent webhooks**: the compare-and-set update plus webhook-ID deduplication. **Duplicate refund requests**: a
unique constraint on `(payment_id, client_request_key)`.

---

## Part V Review — Interview mode

1. **Unrepresentable invalid states:** counting (Chapter 5.1 §2, question 2). Deliberately *not* doing it: when
   combinations would explode (independent flags belong in a product of small types), when transitions are
   configuration-driven, or for data you only pass through unchanged.
2. **Parse, don't validate, for a Java team:** a boundary layer converts raw input into domain types once, and the core
   accepts only domain types, so the `Validators` utility class disappears (Chapter 5.1 §9, Meridian onboarding). The
   structural change is that a new entry point *cannot* skip validation.
3. **Exhaustiveness:** Chapter 5.1 §4 and question 8.
4. **Default layout vs `repr(C)`:** Chapter 5.2, question 2. `repr(C)` everywhere means larger structs (32 vs 24 bytes,
   verified), blocked layout optimizations, and an open invitation to write structs to disk.
5. **`Option<Option<&u8>>`:** 16 bytes (verified in `review-answer-sizes.rs`). `&u8` has exactly one niche (null), so
   `Option<&u8>` is 8, and the outer `Option` needs a tag. rustc doesn't use alignment niches, and `&u8`'s alignment is
   1 anyway.
6. **FFI-safe niches:** Chapter 5.2, question 5.
7. **Bytes to disk:** padding bytes are uninitialized, so writing them leaks memory contents and makes checksums
   nondeterministic (Chapter 5.2 §10). Also: endianness, alignment rules, and layout changing with the code.
8. **Newtypes are free:** the `discount_typed = discount_raw` alias, identical `shard_*` code, equal `size_of`, and the
   `repr(transparent)` guarantee (Chapter 5.3 §6).
9. **Typed-ID marker:** Chapter 5.3, question 4.
10. **Derived bounds:** Chapter 5.3's debugging exercise.
11. **Connection:** listing `ch04-01-connection.rs`. The socket lives in `Connected` and `Authenticated`, the session
    only in `Authenticated`. A failed `authenticate` returns `(Connection<Connected>, AuthError)`, handing the
    connected handle back.
12. **Moves:** Chapter 5.4, the Analogy limit in §8.
13. **Persist and reload:** Chapter 5.4, question 6.
14. **Where to use it in payments:** in-process protocols such as the PSP client handshake, the capture call path,
    required-field request builders, and database transaction handles. Refuse it for the persisted lifecycle as a whole,
    for configuration-driven workflows, and for state driven by UI or other services. Those get the hybrid boundary.
15. **"Solves double charging":** it removes in-process reuse of a stale authorization (E0382) and forces the timeout
    case to be handled explicitly in the types. It doesn't resolve remote uncertainty, crashes, or concurrency. Those
    need idempotency keys, compare-and-set, and reconciliation (Chapter 5.4 §10).
