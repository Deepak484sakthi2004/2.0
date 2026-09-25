# Part VIII — Error Handling

> **Part question:** *How should a failure travel: from the line where it happens, through the functions and crates
> above it, across a thread or FFI boundary, and out of the service to a client, a log, and a pager, so that every
> party gets exactly the information it needs to act?*

Parts I–VII kept promising "Part VIII covers that": the `String` errors in Chapter 2.4's frame parser and Project L1's
`logstat`, the `?` operator used before it was explained, the panic strategy chosen per system in Chapter 2.2, and the
question Chapter 3.5 left open, *what do you do when cleanup itself can fail, given that `Drop` can't?* This Part pays
those debts.

It moves outward in four steps. **Values:** `Option` and `Result`, and what `?` really is (verified in MIR: two trait
calls, a branch, and a return; in release assembly, one `test` and one jump). **Types:** designing errors as APIs, with
variants for programs and messages for people, `thiserror` for libraries, and `anyhow` for applications, plus what each
costs in allocations, sizes, and generated code. **Bugs:** panics, the unwinder, landing pads, poisoning, double panics,
the `extern "C"` abort, and how to contain a bug at a request, task, or FFI boundary. **Boundaries:** where most real
error-handling outages happen: retries, idempotency, ambiguous timeouts, status codes, logs, and metrics.

Throughout, the comparison is three-way: **Java exceptions** (checked and unchecked, stack traces captured on every
throw), **Go error returns** (explicit but ignorable, no sum types), and **Rust's `Result`** (explicit, typed, and
cheap), with an honest account of where each model's architectural consequences show up.

## Chapter map

```text
8.1 Result, Option, and ?        failure as a value; ? desugared (Try::branch / FromResidual, verified MIR);
                                 the two E0277s; Result sizes and niches; the happy path of ? in real assembly
      │
8.2 Designing Error Types        an error type is an API; the chain rule (Display vs source); thiserror (expansion
                                 shown) vs anyhow; Box<dyn Error> and anyhow allocation counts; a 520-byte Result
                                 in assembly (sret + memcpy); header parser and logstat redesigned
      │
8.3 Panics, Unwinding, Abort     Result vs panic; hook → unwinder → landing pad (verified asm); catch_unwind at
                                 request, task, and FFI boundaries; poisoning; double panic and extern "C" aborts;
                                 exit statuses; panic cost measured; a commit that can fail
      │
8.4 Errors at Service Boundaries whose fault / did it happen / retry? / who must know; payments taxonomy with an
                                 exhaustive mapping; backoff + jitter + deadlines; idempotency keys; why timeouts
                                 are ambiguous at the socket; logging and metrics by class
      │
Part VIII Review                 review a refunds PR with 13 error-handling defects; interview mode
```

## What you'll be able to do after Part VIII

- Choose between `Option`, `Result`, `Result<Option<T>, E>`, and a panic for any failure, and defend the choice.
- Explain exactly what `?` compiles to, and read every error it produces.
- Design library error enums that callers can act on and applications can report, with correct cause chains.
- Predict and measure what an error representation costs: size on every return, allocations on failure, copies per
  propagation layer.
- Contain bugs at the right boundary, pick `unwind` or `abort` per system, and design cleanup that can fail.
- Build a service error boundary: a classified taxonomy, stable codes, safe client bodies, retries that can't
  double-charge, and logs and metrics that page for the right things.

## Promises kept from earlier Parts

| Promise | Where |
|---|---|
| Replace `String` errors in the frame-header parser (Chapters 2.4/2.5) with an error enum | 8.2 §1, §3, §10 |
| Replace `String` errors in Project L1 `logstat`; explain `?` properly | 8.1 §3–§4; 8.2 §9 |
| Panic strategy per system: Tokio unwind, FFM library unwind + `catch_unwind`, CLI abort (Chapter 2.2) | 8.3 §7 (all three verified) |
| Unwind cleanup blocks (Chapter 3.5 MIR) → what they become in machine code | 8.3 §4 (landing-pad assembly) |
| `Drop` can't fail → an explicit, fallible close; `commit` returning `Result` (Chapter 3.5 exercise) | 8.3 §7 |

## Listings

`listings/part-08/`: 38 files, 42 checks, all verified on rustc 1.98.1 (edition 2024), including two runs under
**Miri**, `thiserror`, `anyhow`, `tracing`, `serde_json`, `tempfile`, and Tokio listings, and five `crash` checks
(aborts, and `main` returning `Err`).
Compiler artifacts: the MIR of `?`; release assembly of `?`'s happy path, of `?` with a large error, and of a landing
pad; and the `thiserror` macro expansion.
