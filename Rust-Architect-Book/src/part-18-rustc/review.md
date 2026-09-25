# Part XVIII Review — The Compiler Detective & Interview Mode

> Consolidate Part XVIII, then use it: read a small service's compiler artifacts and explain every line that matters,
> then close three build tickets using the stage and query that produced each symptom. Then answer senior-level
> questions without notes. Answers are in **Appendix A, Part XVIII**.

---

## Part XVIII on one page

```text
 ENGINE        rustc = a database of memoized, dependency-tracked QUERIES keyed by DefId (per session) / DefPathHash
               (across sessions); errors are results; cycles are E0391; red-green + early cutoff = incremental
               gating: resolution → per-body typeck → borrowck only for well-typed bodies → crate-wide passes (privacy,
               lints) only if nothing failed → post-mono errors only in full builds
        │
 FRONT END     tokens → AST → EXPANSION ⟷ RESOLUTION fixpoint → HIR (for/?/while let/.await/format_args desugared,
               elided lifetimes explicit); hygiene: locals at definition site, items at call site, $crate
        │
 TYPES         typeck(body): inference variables + unification; adjustments recorded; method probe
               (autoderef steps × by-value/&/&mut, first step wins); obligations → trait solver
               (candidates → winnow → confirm → nested goals; yes / no / AMBIGUOUS); fallback ({integer}→i32,
               diverging → ! in 2024); E0277 = failed proof, E0275 = endless proof
        │
 THIR / MIR    THIR: typed, fully explicit tree (exhaustiveness, unsafety) → MIR: CFG of basic blocks, explicit
               drops and unwind edges; promote → BORROWCK → drop elaboration → optimize → optimized_mir;
               mir_for_ctfe → const eval = a MIR interpreter (the engine Miri is built on)
        │
 BORROWCK      renumber → MIR type check (outlives constraints) → liveness (drop-live!) → region inference
               (regions = sets of points) → dataflow (loans in scope; moves); runs BEFORE optimization;
               two-phase borrows for autoref receivers; closures propagate requirements; then regions ERASED
               problem case #3: rejected by 1.98.1 stable, accepted by nightly 1.100 (2026-09-24)
        │
 CODEGEN       collector from roots (instances, vtables with every method, drop glue, generic consts) → CGUs →
               layout_of + fn_abi_of (noalias/readonly/nonnull/sret/pair) + symbol_name (v0) → LLVM IR →
               LLVM passes (inlining, merging: unnamed_addr ⇒ distinct fns may share an address) → objects
               backends: LLVM (default), Cranelift (fast debug, nightly), GCC
```

## Ten ideas to carry forward

1. **The compiler is a set of questions, not a sequence of phases.** Every artifact and every error comes from a
   specific query about a specific item, which is why errors hide each other and why incremental builds work.
2. **Fixing one error revealing another is expected**, and predictable from the gating table in Chapter 18.1.
3. **Macros see tokens, the type checker sees types, and nothing sees both.** Derives emit bounds and let the solver
   prove them; `macro_rules!` bodies resolve item names at the call site unless you write `$crate::`.
4. **Trait errors are proofs.** Read an E0277 bottom-up from the leaf goal; treat an E0275 as a design problem, not a
   limit to raise.
5. **MIR is the place to ask "when".** Drops, unwind paths, moves, and lock scopes are explicit there, and only there.
6. **Const evaluation is an interpreter you can use**: invariants of shipped tables belong in `const` assertions.
7. **The borrow checker reads unoptimized MIR, on purpose.** Its verdicts depend on the language rules, never on the
   optimizer, and a `Drop` impl is a use.
8. **Adding `Drop`, a lifetime parameter, or `&mut self` to a public type changes what callers can compile.** Treat
   them as breaking changes.
9. **`noalias` is a chain**: borrow checker → ABI computation → LLVM attribute → one load instead of two. And
   function addresses are not identities.
10. **Debug artifacts answer semantic questions; release artifacts answer performance questions.** Pick the earliest
    stage that answers the question, and the right profile for it.

---

## Capstone: the compiler detective

### Part A: read a service's artifacts

A small Meridian ledger client posts balance changes and writes a journal line per posting through a `dyn Sink`
(listing `review-ledger-client.rs`, verified in debug and release):

```rust
use std::fmt;

macro_rules! ensure {
    ($cond:expr, $err:expr) => {
        if !$cond {
            return Err($err);
        }
    };
}

#[derive(Debug)]
pub enum LedgerError {
    Overflow,
    Negative(i64),
    Closed,
}

impl fmt::Display for LedgerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LedgerError::Overflow => write!(f, "balance overflow"),
            LedgerError::Negative(b) => write!(f, "would go negative: {b}"),
            LedgerError::Closed => write!(f, "account closed"),
        }
    }
}

pub trait Sink {
    fn write(&mut self, line: &str);
}

pub struct Stdout;

impl Sink for Stdout {
    fn write(&mut self, line: &str) {
        println!("  sink: {line}");
    }
}

pub struct Account {
    pub id: u64,
    pub balance: i64,
    pub open: bool,
}

pub struct Journal<'a> {
    sink: &'a mut dyn Sink,
    entries: u32,
}

impl Drop for Journal<'_> {
    fn drop(&mut self) {
        let line = format!("journal closed after {} entries", self.entries);
        self.sink.write(&line);
    }
}

#[inline(never)]
pub fn post(acct: &mut Account, delta: i64, journal: &mut Journal<'_>) -> Result<i64, LedgerError> {
    ensure!(acct.open, LedgerError::Closed);
    let next = acct.balance.checked_add(delta).ok_or(LedgerError::Overflow)?;
    ensure!(next >= 0, LedgerError::Negative(next));
    acct.balance = next;
    journal.entries += 1;
    journal.sink.write(&format!("acct {} -> {}", acct.id, next));
    Ok(next)
}

pub trait Currency {
    const MINOR: i64;
}
pub struct Eur;
pub struct Usd;
impl Currency for Eur {
    const MINOR: i64 = 100;
}
impl Currency for Usd {
    const MINOR: i64 = 100;
}

#[inline(never)]
pub fn to_major<C: Currency>(minor: i64) -> i64 {
    minor / C::MINOR
}

fn main() {
    let mut out = Stdout;
    let mut acct = Account { id: 7, balance: 1_000, open: true };
    {
        let mut j = Journal { sink: &mut out, entries: 0 };
        println!("{:?}", post(&mut acct, -300, &mut j));
        println!("{:?}", post(&mut acct, -900, &mut j));
    }
    out.write("done");
    let eur: fn(i64) -> i64 = to_major::<Eur>;
    let usd: fn(i64) -> i64 = to_major::<Usd>;
    println!("eur={} usd={} same fn? {}", eur(12_345), usd(12_345), std::ptr::fn_addr_eq(eur, usd));
}
```

Output in debug:

```text
  sink: acct 7 -> 700
Ok(700)
Err(Negative(-200))
  sink: journal closed after 1 entries
  sink: done
eur=123 usd=123 same fn? false
```

Output in release: identical, except the last line ends in `same fn? true`.

**Artifact 1: the HIR of `post`** (`-Target hir -CrateType bin`, verbatim):

```text
fn post(acct: &'_ mut Account, delta: i64, journal: &'_ mut Journal<'_>)
    ->
        Result<i64,
        LedgerError> {
    if !acct.open { return Err(LedgerError::Closed); };
    let next =
        match branch(acct.balance.checked_add(delta).ok_or(LedgerError::Overflow))
            {
            Break {  0: residual } => #[allow(unreachable_code)]
                return from_residual(residual),
            Continue {  0: val } => #[allow(unreachable_code)]
                val,
        };
    if !(next >= 0) { return Err(LedgerError::Negative(next)); };
    acct.balance = next;
    journal.entries += 1;
    journal.sink.write(&::alloc::__export::must_use({
                    ::alloc::fmt::format({
                            super let args = (&acct.id, &next);
                            super let args =
                                [format_argument::new_display(args.0),
                                        format_argument::new_display(args.1)];
                            unsafe {
                                format_arguments::new(b"\x05acct \xc0\x04 -> \xc0\x00",
                                    &args)
                            }
                        })
                }));
    Ok(next)
}
```

**Artifact 2: the release assembly of `post`** (`-Target asm -Mode release -CrateType bin`, verbatim, the unwind path
at the end trimmed):

```text
playground::post:
	push	rbp
	push	r15
	push	r14
	push	r13
	push	r12
	push	rbx
	sub	rsp, 72
	cmp	byte ptr [rsi + 16], 0
	je	.LBB7_1
	add	rdx, qword ptr [rsi + 8]
	jo	.LBB7_13
	mov	qword ptr [rsp + 8], rdx
	js	.LBB7_5
	mov	r13, rdi
	mov	r12, rdx
	mov	qword ptr [rsi + 8], rdx
	inc	dword ptr [rcx + 16]
	mov	r15, qword ptr [rcx]
	mov	rbp, qword ptr [rcx + 8]
	mov	qword ptr [rsp + 40], rsi
	mov	rax, qword ptr [rip + <u64 as core::fmt::Display>::fmt@GOTPCREL]
	mov	qword ptr [rsp + 48], rax
	lea	rax, [rsp + 8]
	mov	qword ptr [rsp + 56], rax
	mov	rax, qword ptr [rip + <i64 as core::fmt::Display>::fmt@GOTPCREL]
	mov	qword ptr [rsp + 64], rax
	lea	rsi, [rip + .Lanon.094a2e71abecd2edc154eaae0f6063c8.5]
	lea	rdi, [rsp + 16]
	lea	rdx, [rsp + 40]
	call	qword ptr [rip + alloc::fmt::format::format_inner@GOTPCREL]
	mov	rbx, qword ptr [rsp + 16]
	mov	r14, qword ptr [rsp + 24]
	mov	rdx, qword ptr [rsp + 32]
	mov	rdi, r15
	mov	rsi, r14
	call	qword ptr [rbp + 24]
	test	rbx, rbx
	je	.LBB7_12
	mov	edx, 1
	mov	rdi, r14
	mov	rsi, rbx
	call	qword ptr [rip + __rustc::__rust_dealloc@GOTPCREL]

.LBB7_12:
	mov	qword ptr [r13 + 8], r12
	mov	qword ptr [r13], -1
	jmp	.LBB7_2

.LBB7_1:
	mov	qword ptr [rdi], 2
	jmp	.LBB7_2

.LBB7_5:
	mov	qword ptr [rdi], 1
	mov	qword ptr [rdi + 8], rdx

.LBB7_2:
	add	rsp, 72
	pop	rbx
	pop	r12
	pop	r13
	pop	r14
	pop	r15
	pop	rbp
	ret

.LBB7_13:
	mov	qword ptr [rdi], 0
	jmp	.LBB7_2
```

**Artifact 3: two nightly dumps** (listing `review-ledger-client-dumps.rs`, verified). The vtable for `Stdout as Sink`:

```text
error: vtable entries: [
           MetadataDropInPlace,
           MetadataSize,
           MetadataAlign,
           Method(<Stdout as Sink>::write),
       ]
```

and the ABI rustc computes for `post` in release. The dump listing returns `Result<i64, ()>` to keep the output short,
so its return value is a `Pair` in two registers, where the real `Result<i64, LedgerError>` is returned indirectly
(Artifact 2 writes it through `rdi`). The arguments are the same (trimmed):

```text
error: fn_abi_of(post) = FnAbi {
           args: [
               ArgAbi { ty: &mut Account, …  regular: NoAlias | NonNull | NoUndef | NoFree, … },
               ArgAbi { ty: i64, …           regular: NoUndef, … },
               ArgAbi { ty: &mut Journal<'_>, … regular: NoAlias | NonNull | NoUndef | NoFree, … },
           ],
           ret: ArgAbi { ty: Result<i64, ()>, … mode: Pair( … ) },
           conv: Rust,
           can_unwind: true,
```

**Questions for Part A.** Answer each from the artifacts, naming the stage that produced what you point to.

1. Where did `ensure!` go? Why is `!$cond` printed as `!acct.open` in one place and `!(next >= 0)` in the other, and
   which chapter's rule explains the parentheses?
2. Find the `?` in the HIR. Which trait methods does it call, and what does `from_residual` do for this `Result`?
3. Map the first eight instructions of `post` to source lines. Which instruction is `checked_add`'s overflow test, which
   is `next >= 0`, and where did `ok_or(LedgerError::Overflow)` go?
4. From the assembly alone, give the field offsets of `open` and `balance` in `Account`, and of `entries` in `Journal`.
   Which of those offsets are guaranteed by the language?
5. Explain `call qword ptr [rbp + 24]`: what's in `rbp`, why offset 24, and how does Artifact 3 confirm it?
6. The function writes `2`, `1` (plus a payload), `0`, or `-1` to `[rdi]` before returning. Decode the encoding of
   `Result<i64, LedgerError>`. Which part of it is a niche (Chapter 5.2), and is any of it a promise?
7. Why does the `format!` in `post` show up as a call to `format_inner` followed by a conditional `__rust_dealloc`?
   What would change if the journal line were built into a reused buffer (Chapter 9.2)?
8. Artifact 3 marks both references `NoAlias` but the `i64` only `NoUndef`. Why the difference? And `can_unwind: true`
   means Artifact 2's trimmed tail contains a landing pad: which call can unwind into it, what must it clean up, and how
   does it end (Chapter 8.3)?

### Part B: three build tickets

**Ticket 1: "the config crate won't compile after a refactor."** A teammate replaced a nested configuration struct
with a type alias (listing `review-ticket-alias-cycle.rs`, verified):

```rust,compile_fail
use std::collections::HashMap;

type Config = HashMap<String, Config>;
```

```text
error[E0391]: cycle detected when expanding type alias `Config`
 --> src/main.rs:6:31
  |
6 | type Config = HashMap<String, Config>;
  |                               ^^^^^^
  |
  = note: ...which immediately requires expanding type alias `Config` again
  = note: type aliases cannot be recursive
  = help: consider using a struct, enum, or union instead to break the cycle
note: cycle used when checking that `Config` is well-formed
```

(Trimmed.) Which mechanism reports this, which query is on the cycle, and why does a `struct Config(HashMap<String,
Config>)` compile when the alias doesn't? What does the struct cost at run time?

**Ticket 2: "removing a pair of braces broke the build."** A cleanup PR removed the braces around the journal in
`main` (listing `review-ledger-client-nobraces.rs`, verified):

```text
error[E0499]: cannot borrow `out` as mutable more than once at a time
  --> src/main.rs:32:5
   |
30 |     let mut j = Journal { sink: &mut out, entries: 0 };
   |                                 -------- first mutable borrow occurs here
31 |     j.entries += 1;
32 |     out.write("done");
   |     ^^^ second mutable borrow occurs here
33 | }
   | - first borrow might be used here, when `j` is dropped and runs the `Drop` code for type `Journal`
```

Explain the error with Chapter 18.5's steps. Why did the braces matter, and what would happen to the program's output
if `Journal` had no `Drop` impl? Give two fixes, and say which one keeps the "journal closed" line in the same place in
the output.

**Ticket 3: "a test passes in debug and fails in release."** A new test asserts that the EUR and USD converters are
different functions (`assert!(!fn_addr_eq(eur, usd))`). It passes under `cargo test` and fails in the release test
job. The release assembly of `main` contains:

```text
	mov	edi, 12345
	call	playground::to_major::<playground::Eur>
	mov	qword ptr [rsp + 80], rax
	mov	edi, 12345
	call	playground::to_major::<playground::Eur>
	mov	qword ptr [rsp + 8], rax
	mov	byte ptr [rsp + 6], 1
```

and no `to_major::<Usd>` anywhere. What happened, at which stage, and what does `mov byte ptr [rsp + 6], 1` tell you
about the comparison? Is the test wrong, the compiler wrong, or the design wrong? Rewrite the test to check what it
actually cares about.

Write your answers before opening Appendix A. For each one, say which stage and which artifact proves it, the way
Part XVIII's chapters did.

---

## Interview mode

*Senior-level. Answer aloud or in writing, without notes, before checking Appendix A.*

### Language

1. Which of these are language guarantees and which are rustc implementation details: drop order, the layout of a
   default-repr struct, `noalias` on `&mut`, function address uniqueness, two-phase borrows, never-type fallback?
2. What does hygiene guarantee for `macro_rules!`, and what doesn't it? How do you write a macro that's robust to its
   call site?
3. Why are item signatures never inferred in Rust? What would break if they were?

### Compiler

4. Explain rustc's query system to a Java engineer who knows javac's phases. Why does it make errors hide each other,
   and how does it make incremental compilation work?
5. Walk through the life of a function from HIR to object code, naming each representation and one check or decision
   that happens on it.
6. How does the borrow checker compute a loan's lifetime? Why does it run on unoptimized MIR, and what's different
   about Polonius?
7. What does the monomorphization collector do, and why does a `dyn Trait` cast pull in code you never call?

### Performance

8. Where does rustc spend time on a trait-heavy crate versus a generic-heavy crate, and what tools show it?
9. A colleague benchmarks a hot path in a debug build and concludes `String::len` is a function call worth caching.
   What do you show them, from which artifacts?
10. What does `noalias` buy, where does it come from, and when is it absent?

### Architecture

11. Which changes to a public type are breaking because of how rustc checks callers (borrowck, inference, coherence)?
    Give one example per mechanism.
12. Design a macro and code-generation policy for a company with thirty Rust services: proc macros, `build.rs`,
    checked-in generated code, review, and build-time budgets.
13. How do you keep release-only behavior differences (overflow checks, function merging, `debug_assert!`) from hiding
    bugs until production?

### Tooling

14. For each question, name the artifact and profile you'd use: when is this lock released; does this loop allocate;
    which impl did this method call resolve to; is this match exhaustive by construction; why is this crate slow to
    compile.
15. How would you use the Rust Playground, a local nightly, and the pinned stable toolchain differently when
    investigating compiler behavior? What can each tell you, and what can't it?

---

## Looking ahead: Part XIX

Every artifact in this Part ended at an object file: the `call qword ptr [rip + foo@GOTPCREL]` indirections, the v0
symbols with their crate hashes, the `personality` routine and landing pads, `lang_start` wrapping `main`, the
`__rust_alloc` shims. Part XIX follows those objects the rest of the way: relocations and symbols, static and dynamic
linking, ELF and its sections (including the unwind tables Chapter 8.3 relied on), how the OS turns an executable into
a process, virtual memory and `mmap`, and the system calls underneath `std`.
