# Part IV — The Borrow Checker

> **Part question:** *What exactly does the borrow checker prove, how does it prove it, where does it give up, and how
> do you turn each of its error messages into a precise statement about your program's ownership?*

Part III used the borrow checker's verdicts. Part IV opens the checker. You'll learn the model it works with (places,
loans, and conflicting accesses), how it decides when a loan ends (liveness, not scopes), what lifetime annotations
really say (relationships, never durations), why some types can be substituted for others and some can't (variance),
how to write APIs that lend data to callbacks without letting them keep it (higher-ranked bounds), and finally the skill
this book has been building toward since Chapter 1.1: **reading any borrow-checker error as an ownership proof.**

The Part also shows the checker's limits honestly. Some correct programs are still rejected (verified on rustc 1.98.1:
the classic "conditional return of a borrow" case), and some wrong programs slip past when `unsafe` is used to silence
it. A program that "works" and prints the right answer is shown, under **Miri**, to be undefined behavior.

## Chapter map

```text
4.1 Aliasing XOR Mutation          places, loans, conflicts; iterator invalidation; UnsafeCell as the one principled
                                   exception (verified in LLVM IR: `&Cell<i32>` loses `noalias`)
      │
4.2 NLL, Liveness, Reborrowing     loans end at last use, per path; reborrows as nested loans; the generic-parameter
                                   trap; the case NLL still rejects, and what Polonius would change
      │
4.3 Lifetime Annotations & Elision annotations relate, never extend; elision's three rules; structs that borrow;
                                   `'static` (both meanings); the leak that "fixes" a 'static bound
      │
4.4 Variance and Subtyping         why &'static fits where &'a is expected, why &mut can't be covariant, function
                                   contravariance, and Java's covariant arrays as the runtime counterexample
      │
4.5 Higher-Ranked Trait Bounds     for<'a>: "for every lifetime", callbacks that can't keep what they're lent
      │
4.6 Reading Errors as Proofs       the A–B–C method; an error-code translation table; five fix strategies;
                                   what happens when you bypass the checker (Miri catches it)
      │
Part IV Review                     triage a program with four different borrow errors; interview mode
```

## What you'll be able to do after Part IV

- Predict the borrow checker's verdict on a piece of code before compiling it, and explain it in terms of places and
  loans.
- Choose among compile-time exclusivity, `Cell`, `RefCell`, locks, and atomics for shared mutable state.
- Write function and struct signatures with the right lifetime relationships, including when elision gets them wrong
  for you.
- Explain variance, and design types whose variance doesn't make their users' lives harder.
- Design callback and visitor APIs with higher-ranked bounds.
- Triage any borrow error into one of five fix strategies, and justify the one you choose.

## Listings

`listings/part-04/`: 34 files, 36 checks, all verified on rustc 1.98.1 (edition 2024), including two runs under
**Miri**, the interpreter that detects undefined behavior (`tools/verify.ps1` now supports `miri` checks).
