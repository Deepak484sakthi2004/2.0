# Chapter 5.4 — The Type-State Pattern

> **Where this sits:** Part V · Types as Architecture · chapter 4 of 4
> **Prerequisites:** Chapter 2.5 (the payment state machine), Chapters 3.1–3.2 (moves), Chapters 5.1–5.3.
> **After this chapter you can:** encode a state machine so that invalid *transitions* fail to compile; choose between
> state markers (`PhantomData`) and data-carrying states; close the set of states with a sealed trait; build a
> type-state builder with required fields; connect the typed core to run-time data (database rows, queue events) without
> duplicating the transition logic; and say honestly what type-state can't guarantee in a distributed system.

---

## Pass 1 · User level — *Making the wrong call impossible to write*

### 1. Problem

Chapter 5.1 removed invalid *states*: with `enum ConnState { Disconnected, Connected, Authenticated }`, the nonsense
combination "authenticated but not connected" can't be represented. Invalid *transitions* are still possible. Chapter
2.5's payment state machine was a good run-time design:

```rust,ignore
fn next(state: State, event: Event) -> Result<State, String> {
    match (state, event) {
        (Pending, Authorize) => Ok(Authorized),
        (Pending | Authorized, Fail) => Ok(Failed),
        (Authorized, Capture) => Ok(Captured),
        (Captured, Refund) => Ok(Refunded),
        (s @ (Refunded | Failed), e) => Err(format!("{s:?} is terminal; rejected {e:?}")),
        (s, e) => Err(format!("invalid transition {s:?} + {e:?}")),
    }
}
```

(excerpt of listing `ch05-04-state-machine.rs` from Part II). It's exhaustive, and it fails closed. But `next(Pending,
Refund)` still *compiles*, and the mistake shows up as `Err("invalid transition Pending + Refund")` at run time, in
production, possibly in a code path no test exercised. The brief for this Part put it plainly: instead of a
`Connection { connected: bool }`, represent `Disconnected`, `Connected`, and `Authenticated` **as different types**, so
that calling `query` on an unauthenticated connection isn't an error to handle. It's a program that doesn't compile.

### 2. Mental model

**Each state is a type. Each transition is a function that consumes the old state and returns the new one.**

```text
 run-time state machine (Chapter 2.5)            type-state (this chapter)

   one type: Payment { state: State, .. }         one type PER STATE: Payment<Pending>, Payment<Authorized>, ...
   one function: next(state, event) -> Result     one method PER TRANSITION, defined only on its source state:

   ┌─────────┐ Authorize ┌────────────┐            impl Payment<Pending>    { fn authorize(self) -> Payment<Authorized> }
   │ Pending ├──────────►│ Authorized │            impl Payment<Authorized> { fn capture(self)   -> Payment<Captured>   }
   └────┬────┘           └─────┬──────┘            impl Payment<Captured>   { fn refund(self)    -> Payment<Refunded>   }
        │ Fail          Capture│  │Fail
        ▼                      ▼  ▼                 p.refund() on a Payment<Pending>: there is no such method → E0599
   ┌────────┐ ◄──────── ┌──────────┐ Refund ┌──────────┐
   │ Failed │           │ Captured ├───────►│ Refunded │   p used after p.capture(): p was moved → E0382
   └────────┘           └──────────┘        └──────────┘
```

Two language features do all the work:

1. **Impl blocks for specific instantiations.** `impl Payment<Pending> { .. }` adds methods only to `Payment<Pending>`.
   A method that isn't defined for the current state doesn't exist, so calling it is a *name-resolution* failure, not a
   run-time branch.
2. **Moves** (Chapters 3.1–3.2). A transition takes `self` by value, so after `let c = p.capture();`, `p` is gone. No
   stale handle to the old state survives. A language with only shared references (Java, most GC languages) can build
   the types, but can't take the old handle away. That's the important difference, and §8 comes back to it.

**A little history.** "Typestate" is older than Rust: Strom and Yemini introduced it in 1986 as a compiler analysis that
tracks which operations are legal on a variable at each program point. Early Rust (2010–2012) had typestate built in:
predicates declared on functions, and `check` statements the compiler tracked through the code. It was removed in 2012,
years before 1.0. It was complex, rarely used, and most of its value turned out to be reachable with ordinary types plus
affine ownership. Part of it survives in a form you use daily: rustc still tracks, per program point, whether each local
is initialized or moved-out (errors E0381 and E0382). The type-state *pattern* in this chapter is the library-level
successor: no special compiler feature, only generics, impl blocks, and moves.

### 3. Rust code

**The Connection from the brief** (listing `ch04-01-connection.rs`, verified). Here each state *carries the data that
exists in that state*: a socket only once connected, a session only once authenticated.

```rust,ignore
    // The three states. Each carries exactly the data that exists in that state.
    pub struct Disconnected;
    pub struct Connected {
        socket: Socket,
    }
    pub struct Authenticated {
        socket: Socket,
        session: SessionToken,
    }

    pub struct Connection<S> {
        addr: String,
        state: S,
    }

    // Available in every state.
    impl<S> Connection<S> {
        pub fn addr(&self) -> &str {
            &self.addr
        }
    }

    impl Connection<Disconnected> {
        pub fn new(addr: &str) -> Self {
            Connection { addr: addr.to_string(), state: Disconnected }
        }

        /// Consumes the disconnected handle. On failure, hands it back so the caller can retry.
        pub fn connect(self) -> Result<Connection<Connected>, (Connection<Disconnected>, ConnectError)> {
            if self.addr.ends_with(":0") {
                return Err((self, ConnectError::Refused));
            }
            let socket = Socket { fd: 3 };
            Ok(Connection { addr: self.addr, state: Connected { socket } })
        }
    }

    impl Connection<Connected> {
        pub fn authenticate(self, user: &str, password: &str) -> Result<Connection<Authenticated>, (Connection<Connected>, AuthError)> {
            if password.len() < 8 {
                return Err((self, AuthError::BadCredentials));
            }
            let session = SessionToken(format!("sess-{user}-{}", self.state.socket.fd));
            Ok(Connection { addr: self.addr, state: Authenticated { socket: self.state.socket, session } })
        }

        pub fn close(self) -> Connection<Disconnected> {
            Connection { addr: self.addr, state: Disconnected }
        }
    }

    impl Connection<Authenticated> {
        /// Only an authenticated connection can run queries: there is no "not authenticated" error path.
        pub fn query(&mut self, sql: &str) -> Vec<String> {
            vec![format!("[{} via fd {}] {sql}", self.state.session.0, self.state.socket.fd)]
        }

        pub fn close(self) -> Connection<Disconnected> {
            Connection { addr: self.addr, state: Disconnected }
        }
    }
```

```text
connect to 10.0.0.5:0 failed: Refused; retrying
auth failed: BadCredentials; still connected to 10.0.0.5:5432
[sess-ledger-3 via fd 3] SELECT balance FROM accounts WHERE id = 7
closed: Connection { addr: "10.0.0.5:5432", state: Disconnected }
connected and closed without authenticating: 10.0.0.6:5432
size_of: Connection<Disconnected>=24 Connection<Authenticated>=56
```

Look at the **failure types**. A transition that can fail returns `Result<NextState, (SameState, Error)>`. It gives the
caller back the old state, because `self` was consumed and the caller would otherwise lose the connection. That's how
a retry loop keeps its handle: "auth failed ... still connected."

Forgetting to authenticate (listing `ch04-02-query-unauthenticated.rs`, verified):

```rust,ignore
    let mut conn = Connection::new("10.0.0.5:5432").connect();
    // Forgot to authenticate:
    println!("{}", conn.query("SELECT 1"));
```

```text
error[E0599]: no method named `query` found for struct `Connection<Connected>` in the current scope
35 |     println!("{}", conn.query("SELECT 1"));
   |                         ^^^^^ method not found in `Connection<Connected>`
   = note: the method was found for `Connection<Authenticated>`
```

The compiler even says where the method *does* exist, which is a precise description of the missing transition.

**The payment state machine from Chapter 2.5, as types** (listing `ch04-03-payment-typestate.rs`, verified). Payments
carry the same data in every state, so the states are pure **markers**: uninhabited enums behind `PhantomData`, closed
with a sealed trait:

```rust
mod payment {
    use std::marker::PhantomData;

    mod sealed {
        pub trait Sealed {}
    }

    /// The set of states is closed: `Sealed` is unnameable outside this module.
    pub trait State: sealed::Sealed {
        const NAME: &'static str;
    }

    // Uninhabited marker types: they exist only at compile time. No value of them can ever be made.
    pub enum Pending {}
    pub enum Authorized {}
    pub enum Captured {}
    pub enum Refunded {}
    pub enum Failed {}

    macro_rules! states {
        ($($s:ident),*) => {$(
            impl sealed::Sealed for $s {}
            impl State for $s { const NAME: &'static str = stringify!($s); }
        )*};
    }
    states!(Pending, Authorized, Captured, Refunded, Failed);

    pub struct Payment<S: State> {
        id: u64,
        cents: i64,
        _state: PhantomData<S>,
    }

    impl<S: State> Payment<S> {
        pub fn id(&self) -> u64 {
            self.id
        }
        pub fn state(&self) -> &'static str {
            S::NAME
        }
        // Private: the only way to change state is one of the transition methods below.
        fn into_state<T: State>(self) -> Payment<T> {
            Payment { id: self.id, cents: self.cents, _state: PhantomData }
        }
    }

    impl Payment<Pending> {
        pub fn new(id: u64, cents: i64) -> Self {
            Payment { id, cents, _state: PhantomData }
        }
        pub fn authorize(self) -> Payment<Authorized> {
            self.into_state()
        }
        pub fn fail(self) -> Payment<Failed> {
            self.into_state()
        }
    }

    impl Payment<Authorized> {
        pub fn capture(self) -> Payment<Captured> {
            self.into_state()
        }
        pub fn fail(self) -> Payment<Failed> {
            self.into_state()
        }
    }

    impl Payment<Captured> {
        pub fn refund(self) -> Payment<Refunded> {
            self.into_state()
        }
        pub fn amount(&self) -> i64 {
            self.cents
        }
    }
    // Payment<Refunded> and Payment<Failed> have no transition methods: they are terminal.
}

use payment::{Payment, Pending};

fn main() {
    let p = Payment::<Pending>::new(42, 4_999);
    println!("#{} {}", p.id(), p.state());
    let p = p.authorize();
    println!("#{} --authorize--> {}", p.id(), p.state());
    let p = p.capture();
    println!("#{} --capture--> {} ({} cents)", p.id(), p.state(), p.amount());
    let p = p.refund();
    println!("#{} --refund--> {}", p.id(), p.state());

    let q = Payment::<Pending>::new(43, 150).authorize().fail();
    println!("#{} --authorize--> --fail--> {}", q.id(), q.state());
    let r = Payment::<Pending>::new(44, 10).fail();
    println!("#{} --fail--> {}", r.id(), r.state());

    use std::mem::size_of;
    println!(
        "size_of: Payment<Pending>={} Payment<Captured>={} (u64, i64)={}",
        size_of::<Payment<Pending>>(),
        size_of::<Payment<payment::Captured>>(),
        size_of::<(u64, i64)>()
    );
}
```

```text
#42 Pending
#42 --authorize--> Authorized
#42 --capture--> Captured (4999 cents)
#42 --refund--> Refunded
#43 --authorize--> --fail--> Failed
#44 --fail--> Failed
size_of: Payment<Pending>=16 Payment<Captured>=16 (u64, i64)=16
```

Compare the two failure modes with Chapter 2.5's run-time errors. **Refunding a pending payment** (listing
`ch04-04-refund-pending.rs`, verified):

```text
error[E0599]: no method named `refund` found for struct `Payment<Pending>` in the current scope
38 |     let r = p.refund();
   |               ^^^^^^ method not found in `Payment<Pending>`
   = note: the method was found for `Payment<Captured>`
```

**Capturing the same authorization twice** (listing `ch04-05-double-capture.rs`, verified):

```rust,compile_fail
use std::marker::PhantomData;

pub enum Authorized {}
pub enum Captured {}

pub struct Payment<S> {
    id: u64,
    _state: PhantomData<S>,
}

impl Payment<Authorized> {
    pub fn capture(self) -> Payment<Captured> {
        Payment { id: self.id, _state: PhantomData }
    }
}

fn main() {
    let auth: Payment<Authorized> = Payment { id: 42, _state: PhantomData };
    let first = auth.capture();
    // A retry loop that captures the same authorization again (a double charge):
    let second = auth.capture();
    println!("{} {}", first.id, second.id);
}
```

```text
error[E0382]: use of moved value: `auth`
19 |     let auth: Payment<Authorized> = Payment { id: 42, _state: PhantomData };
   |         ---- move occurs because `auth` has type `Payment<Authorized>`, which does not implement the `Copy` trait
20 |     let first = auth.capture();
   |                      --------- `auth` moved due to this method call
22 |     let second = auth.capture();
   |                  ^^^^ value used here after move
note: `Payment::<Authorized>::capture` takes ownership of the receiver `self`, which moves `auth`
```

The first error is the type system: the method isn't there. The second is ownership: the method *was* there, but the
value it needed has been used up. Together they cover both halves of "invalid transition": a transition from the wrong
state, and a transition from a state you already left.

---

## Pass 2 · Systems level — *What the compiler checks, and what's left at run time*

### 4. Under the hood

**E0599 is method resolution failing.** [RUSTC] To resolve `p.refund()`, rustc collects the inherent impls whose self
type unifies with `Payment<Pending>`. `impl Payment<Captured>` doesn't unify (`Pending ≠ Captured`), so `refund` isn't
a candidate, and no trait provides it either. The note "the method was found for `Payment<Captured>`" comes from a
second, diagnostic-only search that ignores the type arguments. It's the compiler helpfully describing the missing
transition.

**E0382 is the move checker**, the same one from Part III, running on MIR. `capture(self)` moves `auth`, and any later
use is a use of moved storage. That's why the markers must *not* be `Copy`, and neither may the payment. A
`#[derive(Clone, Copy)]` on `Payment<S>` would quietly bring back the double capture. (Chapter 5.3's derive trap makes
that less likely than it sounds, because the derive would require the markers to be `Copy`. Don't rely on the accident.
Make it a review rule.)

**Sealing the set of states** (listing `ch04-10-sealed.rs`, verified). Without the seal, downstream code could invent a
state and write its own transitions:

```rust,compile_fail
mod payment {
    use std::marker::PhantomData;

    mod sealed {
        pub trait Sealed {}
    }
    pub trait State: sealed::Sealed {}

    pub enum Pending {}
    impl sealed::Sealed for Pending {}
    impl State for Pending {}

    pub struct Payment<S: State> {
        pub id: u64,
        _state: PhantomData<S>,
    }
}

// Downstream code tries to invent a state that skips authorization.
pub enum AlreadyCaptured {}
impl payment::State for AlreadyCaptured {}

fn main() {}
```

```text
error[E0277]: the trait bound `AlreadyCaptured: Sealed` is not satisfied
22 | impl payment::State for AlreadyCaptured {}
   |                         ^^^^^^^^^^^^^^^ unsatisfied trait bound
note: required by a bound in `State`
 8 |     pub trait State: sealed::Sealed {}
   |                      ^^^^^^^^^^^^^^ required by this bound in `State`
   = note: `State` is a "sealed trait", because to implement it you also need to implement
     `payment::sealed::Sealed`, which is not accessible; this is usually done to force you to use one of the provided
     types that already implement it
```

rustc recognizes the idiom by name. `Sealed` is public (so it can appear in a public trait's bounds) but lives in a
private module (so nobody outside can name it to implement it). Chapter 6.3 covers why this works under coherence rules.
Two more details in the payment listing matter: `into_state` is **private**, so the only public way to change state is
a named transition, and the payment's fields are private, so nobody can build a `Payment<Captured>` with a struct
literal.

**Uninhabited markers.** `enum Pending {}` has zero values (Chapter 5.1's algebra: 0), so nobody can accidentally create
a `Pending` value and pass it around. It exists only to be named in `PhantomData<Pending>`. A unit struct
`struct Pending;` also works and is equally free. The uninhabited enum states the intent more precisely.

**Monomorphization.** [RUSTC] Generic methods like `impl<S: State> Payment<S> { fn id(&self) }` are compiled once *per
state they're used with*. For five states and a few accessor methods, that's a handful of tiny functions, usually
inlined. For a large generic API over many states, it's code size and compile time (Chapter 7.3). A common mitigation
is to put shared logic in non-generic private functions that take the fields, and keep the per-state methods thin.

### 5. Memory

The markers are zero-sized, so every `Payment<S>` has the same layout as the data: `Payment<Pending>` = `Payment<Captured>`
= `(u64, i64)` = 16 bytes (verified above). A transition like `into_state` is a *move* of 16 bytes into a value of a
different type. Chapter 3.1 showed moves compile to register or memory copies that the optimizer usually eliminates, and
§6 shows it disappears here.

Data-carrying states differ: `Connection<Disconnected>` is 24 bytes (the `String` address), and
`Connection<Authenticated>` is 56 (address + socket + session token). **Each state is its own type with its own
layout**, so a transition builds a new value rather than flipping a field. That's usually what you want, because data
that doesn't exist in a state isn't carried as `Option`s you'd have to unwrap. It also means you can't transition "in
place" behind a `&mut`. The idiom for that is Part III's `std::mem::replace`/`take` dance, or an enum wrapper (§7).

The run-time wrapper that §7 needs, `AnyPayment` (listing `ch04-06-persisted.rs`), is 24 bytes: the 16-byte payment
plus a tag, since `(u64, i64)` has no niche. That's the same size the Chapter 2.5 design would have used for
`Payment { id, cents, state: State }`. The typed core didn't cost memory. It moved the tag to the boundary.

### 6. CPU / OS

**The transitions compile to nothing** (listing `ch04-09-typestate-asm.rs`, `emit.ps1 -Target asm -Mode release`,
rustc 1.98.1):

```rust,ignore
/// Two type-state transitions, then read the amount.
#[inline(never)]
pub fn settle_typed(p: Payment<Pending>) -> i64 {
    p.authorize().capture().cents
}

#[inline(never)]
pub fn capture_checked(p: &mut DynPayment) -> Result<i64, &'static str> {
    if p.state != State::Authorized {
        return Err("invalid transition");
    }
    p.state = State::Captured;
    Ok(p.cents)
}
```

```text
playground::settle_typed:              ; Payment<Pending> arrives in two registers: rdi = id, rsi = cents
	mov	rax, rsi                   ; two transitions + a field read = one register move
	ret

playground::capture_checked:           ; the run-time version: load the state, compare, branch, store
	mov	rax, rdi
	cmp	byte ptr [rsi + 16], 1     ; state == Authorized?
	jne	.LBB2_1
	mov	byte ptr [rsi + 16], 2     ; state = Captured
	mov	rdx, qword ptr [rsi + 8]
	xor	ecx, ecx
	mov	qword ptr [rax + 8], rdx   ; Ok(cents)
	mov	qword ptr [rax], rcx
	ret
.LBB2_1:
	lea	rcx, [rip + .Lanon...]     ; Err("invalid transition")
	mov	edx, 18
	...
```

(Labels simplified.) To be honest about what this shows: the run-time check costs a compare and a well-predicted branch,
a nanosecond or less. **Performance is almost never the reason to use type-state.** The reason is the `.LBB2_1` block:
in the run-time design, the invalid transition is a *path* the program can take in production, and it needs error
handling, logging, alerting, and tests. In the typed design, the path doesn't exist, and there's nothing to handle or
test.

The same listing's `settle_dynamic`, a run-time version that sets the state and immediately re-checks it, shows the
optimizer removing the redundant second check. The optimizer can remove checks it can *prove* redundant within one
function. Type-state makes the proof part of the API, across functions and crates, where the optimizer can't see.

---

## Pass 3 · Architect level — *Where type-state fits, and where it stops*

### 7. Trade-offs

**States that come from outside are run-time data.** A payment loaded from PostgreSQL arrives with `state = 'CAPTURED'`
in a column, and an event arrives from a queue. Neither is known at compile time. The answer is a **hybrid**: an enum over
the typed states at the boundary, with every legal (state, event) pair delegating to a typed transition (listing
`ch04-06-persisted.rs`, verified):

```rust,ignore
    /// The run-time view: what a database row or a message can hold.
    pub enum AnyPayment {
        Pending(Payment<Pending>),
        Authorized(Payment<Authorized>),
        Captured(Payment<Captured>),
        Refunded(Payment<Refunded>),
        Failed(Payment<Failed>),
    }

        /// The dynamic edge: an event from a queue. Every legal pair delegates to a typed transition,
        /// so the transition *logic* exists once, in the typed API.
        pub fn apply(self, e: Event) -> Result<AnyPayment, (AnyPayment, String)> {
            use AnyPayment as A;
            use Event as E;
            Ok(match (self, e) {
                (A::Pending(p), E::Authorize) => A::Authorized(p.authorize()),
                (A::Pending(p), E::Fail) => A::Failed(p.fail()),
                (A::Authorized(p), E::Capture) => A::Captured(p.capture()),
                (A::Authorized(p), E::Fail) => A::Failed(p.fail()),
                (A::Captured(p), E::Refund) => A::Refunded(p.refund()),
                (s, e) => {
                    let msg = format!("invalid transition {} + {e:?}", s.state_column());
                    return Err((s, msg));
                }
            })
        }
```

```text
#42 AUTHORIZED --Capture--> CAPTURED
#43 rejected: invalid transition PENDING + Refund; still PENDING
load error: row 44: unknown state "SETTLED"
#45 CAPTURED --Refund--> REFUNDED
#46 PENDING --Fail--> FAILED
#47 rejected: invalid transition REFUNDED + Authorize; still REFUNDED
size_of::<AnyPayment>() = 24
```

This is Chapter 2.5's `match` again, but with one difference that matters: each arm *can only* call transitions that
exist, because `p` in `(A::Pending(p), E::Capture)` is a `Payment<Pending>`, and `p.capture()` wouldn't compile. The
run-time table can't list an illegal transition as legal. Run-time rejection still exists at the boundary, where the
input really is dynamic, and nowhere else. `from_row` is the same idea as Chapter 5.1's `parse`: an unknown state
string is a load error, not a value.

**The decision table:**

| Design | Invalid transitions | Storage and dynamic input | Ergonomics | Use for |
|---|---|---|---|---|
| Run-time enum + `match` (2.5) | run-time `Err` | natural | simple | persisted lifecycles, config-driven workflows |
| Type-state, marker states | compile error | needs a wrapper enum | generic signatures, one type per state | protocols with uniform data |
| Type-state, data-carrying states | compile error; state data can't be missing | needs a wrapper enum | best precision, most types | connections, sessions, handshakes |
| Hybrid (typed core + `AnyPayment` edge) | compile error inside; run-time `Err` only at the edge | natural at the edge | two layers to maintain | long-lived entities with a strict core |

**Where type-state hurts:**

- **Heterogeneous collections.** A `Vec<Payment<?>>` doesn't exist. You need the enum wrapper or a trait object.
- **Branching outcomes.** A transition that can land in two states returns an enum or a `Result`, and the caller
  matches on it. That's precise, and it's more code.
- **Too many states or orthogonal flags.** Two independent booleans as type parameters give four types. The builder
  below uses exactly two flags, and that's about the limit before the signatures stop fitting on a screen.
- **Diagnostics and docs.** rustdoc lists every impl block, and users see `Payment<S>` everywhere. Aliases
  (`type AuthorizedPayment = Payment<Authorized>`) help.
- **Reconfigurable workflows.** If the product team can change allowed transitions without a deploy (Chapter 2.5's
  design exercise), the transitions aren't known at compile time, and type-state can only describe the *maximal* legal
  set.

**Builders with required fields** are type-state's most common everyday form (listings `ch04-07-builder.rs` and
`ch04-08-builder-missing.rs`, verified):

```rust,ignore
    impl<A> RefundBuilder<Missing, A> {
        pub fn payment(self, id: u64) -> RefundBuilder<Set, A> { /* ... */ }
    }
    impl<P> RefundBuilder<P, Missing> {
        pub fn amount(self, cents: i64) -> RefundBuilder<P, Set> { /* ... */ }
    }
    impl<P, A> RefundBuilder<P, A> {
        /// Optional field: allowed in any state, does not change the type.
        pub fn reason(mut self, r: &str) -> Self { /* ... */ }
    }
    impl RefundBuilder<Set, Set> {
        /// `build` exists only when both required fields have been provided.
        pub fn build(self) -> RefundRequest { /* ... */ }
    }
```

```text
refund 1200 cents on payment #42 ("damaged item")
refund 150 cents on payment #43 ("")
```

Setters can come in any order, and optional fields don't change the type. Forgetting one required field:

```text
error[E0599]: no method named `build` found for struct `RefundBuilder<Set, Missing>` in the current scope
36 |     let r = RefundBuilder::new().payment(42).build();
   |                                              ^^^^^ method not found in `RefundBuilder<Set, Missing>`
   = note: the method was found for `RefundBuilder<Set, Set>`
```

The type name *is* the error message: `<Set, Missing>` says the payment is set and the amount is missing. For more than
two or three required fields, a plain constructor with named parameters (a struct of required fields) is usually
clearer than a type-state builder.

### 8. Java comparison

| Need | Java | Rust |
|---|---|---|
| Required fields at construction | Builder whose `build()` throws `IllegalStateException` (Lombok's `@Builder` checks only `@NonNull` fields, at run time) | type-state builder: `build` doesn't exist until the fields are set |
| Enforced call order | **step builder**: each step returns an interface exposing only the next step | impl blocks per state |
| State-dependent operations | `if (!authenticated) throw new IllegalStateException()`; JDBC's `SQLException` on a closed `Connection` | the method exists only on `Connection<Authenticated>` |
| Distinct types per state | possible: `AuthorizedPayment authorize(PendingPayment p)` | the same, plus the old handle is consumed |
| Persisted state | an enum column + `switch` | the hybrid `AnyPayment` boundary |

Java can get surprisingly far. A step builder gives compile-time ordering, and distinct classes per state give
state-specific methods. The type system isn't the obstacle.

> **Analogy limit.** Java has no way to *take away* the old reference. After
> `AuthorizedPayment a = pending.authorize();`, the variable `pending` is still usable, so `pending.authorize()` can run
> a second time, and every other copy of that reference in the heap still points at a "pending" object. Java type-state
> prevents calling the wrong method on a *fresh* value. It can't prevent reusing a *stale* one, which is exactly the
> double-capture bug in §10. Rust's version works because `authorize(self)` consumes the only handle (E0382). The
> pattern depends on affine ownership, not only on the type system.

### 9. Production scenario

**Meridian's payment core.** The payments team rebuilt the capture path in Rust with three layers:

1. **Typed core.** `Payment<S>` with sealed marker states, transition methods consuming `self`, and private fields
   (listing `ch04-03-payment-typestate.rs`). The capture function the PSP adapter exposes has the signature
   `fn capture(p: Payment<Authorized>, psp: &Psp) -> Result<Payment<Captured>, (Payment<Authorized>, PspError)>`. It
   can't be called with anything but an authorized payment, and it can't be called twice with the same one.
2. **Boundary.** `AnyPayment::from_row` parses database rows, and `AnyPayment::apply` maps queue events onto typed
   transitions (listing `ch04-06-persisted.rs`). An unknown state string is a load error with the row ID. An illegal event
   is a structured rejection, counted in metrics, not a panic.
3. **Database compare-and-set.** Every transition is persisted with
   `UPDATE payments SET state = 'CAPTURED' WHERE id = $1 AND state = 'AUTHORIZED'`, and zero rows updated means someone
   else moved the payment first. The type-state guarantees that *this process* follows the state machine. The
   conditional update guarantees that *all processes together* do.

The layering is the architecture lesson. Type-state is a proof about control flow inside one program, and a payment
lives for days across many programs. The typed core removes the class of bugs where code calls the wrong transition.
The boundary contains the run-time uncertainty. The database enforces the machine against concurrency and crashes. None
of the three layers can do another's job.

### 10. Failure scenario

**The double capture.** Meridian's Java `PaymentService.capture(Payment p)` called the PSP, then set
`p.setStatus(CAPTURED)`. A retry wrapper, added for flaky PSP connections, retried on `SocketTimeoutException`. When a
capture request reached the PSP but the response timed out, the wrapper called `capture(p)` again. `p`'s status was
still `AUTHORIZED`, because the first call never returned, so the second capture went through as well. Customers were
charged twice until the PSP's own duplicate detection (which wasn't enabled for all merchants) or a support ticket caught
it.

What each layer of the Rust design does about it:

- **Type-state + moves** make the *in-process* retry impossible to write the naive way. `capture(self)` consumes the
  `Payment<Authorized>`, so a retry loop has to get it back from the error value
  (`Err((payment, PspError::Timeout))`), and the timeout case has to decide explicitly whether the capture may have
  happened. Listing `ch04-05-double-capture.rs` shows the naive version rejected with E0382.
- **But types can't know what the PSP did.** A timeout means "unknown," and no type system can turn an unknown remote
  outcome into a known one. The real fix is an **idempotency key** on the capture request (derived from the payment ID),
  so the PSP deduplicates. The retry middleware's `R: Idempotent` bound from Chapter 5.3 turns "retry only what's
  idempotent" into a compile-time rule.
- **And a crash between the PSP call and the database write** is handled by the database compare-and-set plus a
  reconciliation job that queries the PSP for payments stuck in `AUTHORIZED`.

The honest summary: type-state removed one of the three causes (reusing a stale in-memory handle), and made the other
two *visible* in the code's types. It didn't make distributed failure go away. Anyone presenting type-state as a
solution to double charging is overselling it. What it does is force the code to show where the uncertainty is.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part V).*

1. What two language features make the type-state pattern work in Rust? What happens if you remove either one?
2. Explain how rustc produces E0599 for `p.refund()` on a `Payment<Pending>`. What does the note "the method was found
   for `Payment<Captured>`" come from?
3. When should states be `PhantomData` markers, and when should they be structs carrying data? Give one example of each.
4. Why must a type-state `Payment<S>` not be `Copy`? How could a well-meaning derive bring the bug back?
5. What is a sealed trait, how does the `mod sealed { pub trait Sealed {} }` idiom work, and what does it protect in a
   type-state API?
6. How do you load a type-state entity from a database row? Where does the run-time check live, and why doesn't the
   transition logic get duplicated?
7. Show the assembly-level cost of a type-state transition and of a run-time state check. Why is performance still the
   wrong argument for type-state?
8. What can type-state *not* guarantee for a payment captured through a remote PSP? What handles each remaining risk?

### 12. Exercises

- **Beginner.** Model a file handle: `File<Closed>`, `File<Open>`; `open(self) -> Result<File<Open>, (File<Closed>, Error)>`,
  `read(&mut self)` only when open, `close(self) -> File<Closed>`. Write the compile error for reading a closed file.
- **Intermediate.** Add a `Disputed` state to the payment machine: a `Captured` payment may be disputed, and a dispute
  either reverses (→ `Refunded`) or is rejected (→ back to `Captured`). Add it to the typed core *and* to `AnyPayment`.
  Which of the two did the compiler force you to update?
- **Advanced.** Write a generic `Transaction<'conn, S>` with states `Open`, `Committed`, and `RolledBack`, where
  `commit(self)` and `rollback(self)` consume it. Add a `Drop` impl that rolls back an `Open` transaction. What problem do
  you hit trying to implement `Drop` for only one state, and how do you solve it (hint: Chapter 3.5's guard, and an inner
  `Option`)?
- **Systems.** Emit the assembly for `settle_typed` in debug mode as well as release. How many instructions do the
  transitions cost in debug? What does that tell you about "zero-cost" claims for development builds?
- **Architecture.** Pick a workflow in a system you know (an order, a deployment, a user onboarding). Draw its states
  and transitions. Mark which transitions are in-process (candidates for type-state) and which cross process or time
  boundaries (they need the database and idempotency).

### 13. Debugging exercise

A developer added logging to the typed payment core (listing `ch04-11-derive-debug.rs`, verified):

```rust,compile_fail
use std::marker::PhantomData;

pub enum Pending {} // uninhabited state marker
pub enum Authorized {}

#[derive(Debug)]
pub struct Payment<S> {
    id: u64,
    cents: i64,
    _state: PhantomData<S>,
}

impl Payment<Pending> {
    pub fn new(id: u64, cents: i64) -> Self {
        Payment { id, cents, _state: PhantomData }
    }
    pub fn authorize(self) -> Payment<Authorized> {
        Payment { id: self.id, cents: self.cents, _state: PhantomData }
    }
}

fn main() {
    let p = Payment::new(42, 4_999);
    tracing_like_log(&p); // log the payment before authorizing it
    let _a = p.authorize();
}

fn tracing_like_log<T: std::fmt::Debug>(v: &T) {
    println!("{v:?}");
}
```

```text
error[E0277]: `Pending` doesn't implement `Debug`
25 |     tracing_like_log(&p); // log the payment before authorizing it
   |     ---------------- ^^ the trait `Debug` is not implemented for `Pending`
help: the trait `Debug` is conditionally implemented for `Payment<S>`
 8 | pub struct Payment<S> {
   |                    - unsatisfied requirement introduced here: `Pending: Debug`
   = help: consider manually implementing `Debug` to avoid undesired bounds
help: consider annotating `Pending` with `#[derive(Debug)]`
```

1. Why does `Payment<Pending>` need `Pending: Debug` when no value of `Pending` exists? (Chapter 5.3 §13 is the same
   trap.)
2. The compiler suggests `#[derive(Debug)]` on `Pending`. It works, since deriving `Debug` for an empty enum is fine.
   Why is a manual `impl<S: State> Debug for Payment<S>` still the better fix? Write it so the output includes the state
   name (`S::NAME`).
3. What other derives would a teammate add next, and which one would reintroduce the double-capture bug?

### 14. Design exercise

**Meridian's order lifecycle, revisited.** Chapter 2.5's design exercise had orders with seven states and eleven event
types, and a product team that wants to *disable* transitions per region without a deploy. It compared three designs: an
exhaustive `match`, a configuration-driven table, and a hybrid where the `match` defines the maximal legal set and
configuration can only disable transitions within it. Type-state is the fourth option.

1. Which parts of the order lifecycle happen inside one process's control flow (for example, "place order" creating a
   `Draft` and moving it to `Submitted` in one request), and which happen across time (fulfilment, returns)? Apply
   type-state only to the first kind. Sketch those types.
2. Show how the configuration-disable rule composes with a typed core: where is "is this transition enabled for EU?"
   checked, and what type does a disabled transition return?
3. The mobile team wants to show "which actions are available" for an order. Can the typed core answer that at run time,
   or does the `AnyOrder` boundary need a separate method? Write its signature.
4. Write the ADR paragraph that explains to the Java teams why the Rust order service uses the hybrid, in five
   sentences, including one sentence on what it *doesn't* protect against.
