# Part I — Why Rust Exists

> **Part question:** *Can a language give you C-level control over memory and threads while making whole classes of
> memory and concurrency bugs impossible, without a garbage collector? And if it can, what does that cost?*

Most Rust introductions open with syntax. This one opens with the engineering problem, because every odd-looking rule
you'll meet later (moves, borrows, lifetimes, `Send`, `Sync`, `unsafe`) is a direct answer to it. If you understand the
problem, the rules stop looking arbitrary. If you don't, Rust feels like a compiler that argues with you for no reason.

Part I is deliberately light on syntax. The code you see here is there as evidence. You aren't expected to write it
yet. Parts II–IV teach the syntax and semantics properly.

## Chapter map

```text
1.1 The Problem            What goes wrong in systems code, and the one question every
    Who frees this memory? memory-management design answers.
          │
          ▼
1.2 Five Languages,        C, C++, Java, Go, Rust: five different answers, what each one
    Five Bets              puts in your process, and what each one costs.
          │
          ▼
1.3 Rust's Bet             Ownership, destructive moves, borrowing, lifetimes, Send/Sync,
                           unsafe encapsulation, zero-cost abstractions. What safe Rust
                           guarantees, and precisely what it does NOT.
          │
          ▼
1.4 The Honest Cost        Compile times, learning curve, graph-shaped data, ecosystem gaps,
                           and a decision framework for when Rust is the wrong choice.
          │
          ▼
Part I Review              Architecture review of a real adoption decision, senior-level
                           interview questions, and a capstone ADR.
```

## What you'll be able to do after Part I

- Classify any memory-safety failure precisely: spatial, temporal, initialization, concurrency, or type confusion.
- Explain why undefined behavior is worse than a crash, and why "we run sanitizers in CI" isn't a guarantee.
- Compare C, C++, Java, Go, and Rust by who pays for safety, when, and how much. You'll be comparing mechanisms, not
  slogans.
- State exactly what safe Rust guarantees and what it doesn't: leaks, deadlocks, race conditions, panics, and logic
  errors are all still possible.
- Argue both sides of a "should we use Rust here?" decision with numbers, not enthusiasm.

## Listings for this Part

Every Rust example in Part I is in `listings/part-01/` and verified against rustc 1.98.1 (edition 2024).
