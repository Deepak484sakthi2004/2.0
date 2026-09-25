# Preface: How to Read This Book

## Who this book is for

You already build backend systems. You know Java well enough to have opinions about G1 versus ZGC, you have debugged a
thread pool starved by a blocking call, you have designed for partial failure, and you can reason about consistency models.
You don't need to be taught what a hash map is.

What you want is different: you want to look at a line of Rust and see **through** it — through the compiler, into the
binary, down to what the OS and the CPU actually do — and then climb back up and make an architecture decision with that
knowledge. You want to be the person in the design review who can say *why* Rust does something, what it costs, and when
you should not use it.

This book is written for that goal. It is not a syntax tour, and it is not framework documentation.

## What you will be able to do

By the end you should be able to take any piece of Rust and reason about it at five levels at once:

```text
┌─────────────────────────────┐
│ Architecture                │  Is this the right design? What happens at 10x, 100x load?
├─────────────────────────────┤
│ Distributed System          │  What happens when a node, a disk, or the network fails?
├─────────────────────────────┤
│ Runtime / OS                │  Threads, scheduling, syscalls, virtual memory, file descriptors
├─────────────────────────────┤
│ Compiler / Language         │  What is guaranteed? What did rustc prove, generate, erase?
├─────────────────────────────┤
│ CPU / Memory                │  Where do the bytes live? Which cache line? Which instruction?
└─────────────────────────────┘
```

The path runs **Rust developer → systems engineer → senior engineer → principal engineer → language, compiler, and
systems architect**. The last Part asks you to design and build a small Rust-inspired language. You won't be doing that
to ship a language. You'll be doing it to prove you understand why Rust looks the way it does.

## How the book teaches: three passes

Every major concept is taught three times, at increasing depth:

| Pass | Question | What you get |
|---|---|---|
| **Pass 1 · User level** | *How do I use this?* | The problem it solves, a mental model, idiomatic code |
| **Pass 2 · Systems level** | *What happens in the compiler, memory, runtime, OS, and CPU?* | Mechanism: what is checked, generated, allocated, executed |
| **Pass 3 · Architect level** | *When do I choose this, what are the alternatives, what happens at scale?* | Trade-offs, failure modes, production judgment |

Within those passes, each concept goes through **what → why → how → internals → trade-offs → failure modes → production usage**.
Pass 1 alone would make you a user. Pass 2 alone would make you a trivia collector. You need both of them, applied through
Pass 3, to become an architect.

## Chapter anatomy

Every chapter has the same skeleton, so you always know where you are:

```text
Pass 1 · User level          1. Problem          2. Mental model      3. Rust code
Pass 2 · Systems level       4. Under the hood   5. Memory            6. CPU / OS
Pass 3 · Architect level     7. Trade-offs       8. Java comparison   9. Production scenario   10. Failure scenario
Practice                     11. Interview & architecture questions   12. Exercises (5 levels)
                             13. Debugging exercise                   14. Design exercise
```

Recurring boxes:

> **What actually happens?** Stops the narrative and asks where a value lives, who owns it, when it's destroyed, what the
> compiler knows, what LLVM knows, and what the CPU executes. Over time these questions should become your reflex.

> **Why not X?** The obvious alternative (Java, Go, `clone()` everything, `Arc<Mutex<T>>` everywhere, `unsafe`), and
> the engineering reasons it is or isn't the right call.

> **Analogy limit.** Where a Java comparison stops being true. Analogies are scaffolding. This box is where one comes down.

## Six layers of truth, and the tags that separate them

A lot of confusion about Rust comes from mixing up *what the language guarantees* with *what today's compiler happens to do*.
This book keeps them apart with explicit tags:

| Tag | Meaning | Example |
|---|---|---|
| **[LANG]** | Language semantics. Guaranteed by Rust (the Reference, the "guarantees" sections of std docs). | A moved-from variable cannot be used. |
| **[RUSTC]** | How the current compiler implements it. Can change between releases. | Borrow checking runs on MIR. |
| **[RUNTIME]** | What happens when the program executes. | A panic unwinds the stack and runs destructors. |
| **[OS]** | Operating-system behavior (Linux unless stated). | A new thread's stack is `mmap`ed with a guard page. |
| **[CPU]** | Hardware behavior (x86-64 unless stated). | A `lock xadd` makes an increment atomic. |
| **[LIB]** | Library or ecosystem implementation choice. | `Vec` currently doubles its capacity when it grows. |
| **[VERSION]** | Depends on the edition or compiler version. | NLL borrow checking arrived with the 2018 edition. |

If a sentence has no tag and makes a strong claim, it is a claim about engineering judgment, not about Rust. Argue with it.

## The running case study

Production scenarios take place at **Meridian**, a fictional payments-and-marketplace company. Its backend is mostly Java,
with a C++ market-data service and some Go tooling, and it is deciding where Rust fits. Meridian gives the scenarios
continuity: the API gateway you examine in Part I is the one you benchmark in Part XX and harden in Part XXI.

Starting at Project Level 4 you also build your own system, **Ferrite**. It begins as a concurrent key-value store and
grows through persistence, an async network protocol, a storage engine, and replication into a small distributed
database.

## The project ladder

| Level | Project | Part |
|---|---|---|
| 1 | Production-grade CLI tool | II |
| 2 | Streaming file processor | III |
| 3 | Multithreaded HTTP server from raw TCP | XI |
| 4 | Concurrent key-value store (Ferrite v1) | XI |
| 5 | Async TCP server (Ferrite v2) | XIII |
| 6 | Database client/server protocol | XXII |
| 7 | Persistent storage engine (Ferrite v3) | XXIII |
| 8 | Distributed cache | XXIV |
| 9 | Message broker | XXIV |
| 10 | Mini database (Ferrite v4) | XXIV |
| 11 | Mini distributed database (Ferrite v5) | XXIV |
| 12 | Toy programming language | XXVI |
| 13 | Toy compiler | XXVI |

Each major project ends with an **architecture review**: bottlenecks, allocation sites, contention points, ownership
boundaries, failure model, and what you would change if latency, throughput, or developer velocity became the priority.

## Code, versions, and verification

- **Baseline:** Rust **1.98 stable** (current as of September 2026), **edition 2024**. Anything unstable or
  version-sensitive is tagged **[VERSION]**.
- **Every Rust listing is verified.** Each one exists as a standalone file under `listings/`, with a header saying what
  should happen: runs and prints X, panics with Y, or fails to compile with error `E0502`. `tools/verify.ps1` checks
  all of them against the real compiler on the Rust Playground. When the book shows a compiler error, it's the real
  error from rustc 1.98.1, trimmed only for length.
- Code blocks marked `rust,compile_fail` are *supposed* to be rejected. That rejection is the lesson.
- Line numbers in quoted compiler output refer to the listing files, which start with a one-line `// verify:` header.
  They can therefore be one higher than the snippet printed in the chapter.
- C, C++, Java, and Go snippets are kept short and conventional. They're there for comparison. They aren't meant as
  production code in those languages.

## Numbers

Performance numbers in this book fall into three kinds. Each one is labelled:

1. **Measured.** You get the code and the environment, and you can rerun it.
2. **Order of magnitude.** Things like "an L1 hit costs about a nanosecond, a DRAM miss about 100 ns". These are for
   reasoning. Don't quote them as benchmarks, because they vary by hardware.
3. **Reported by someone else.** Always attributed with who and when ("Cloudflare reported (2022)...") and treated as
   evidence from that context. They aren't universal truths.

The book never says "X is faster than Y" without either a measurement or an exercise that asks you to measure it.
Premature performance claims are as dangerous as premature optimization.

## Answers

Interview questions and exercises have **no answers inline**. Answer keys live in the appendix (Appendix A), one per
Part. Write your answer down first, even if it's two sentences, and then compare. You learn from the gap between your
answer and the key, and reading the key first leaves you with no gap to learn from.

## How to study

- **Predict, then verify.** Before you run a listing, predict the output or the compiler error. Wrong predictions teach
  the most.
- **Use the Playground.** Parts I and II need nothing installed: play.rust-lang.org is enough. Toolchain setup,
  including options for a disk-constrained Windows machine, is covered in Chapter 2.1.
- **Do the debugging exercises in the compiler, not in your head.** Reading a borrow-checker error as an ownership proof
  is a skill, and you only get it by practice.
- **Write the design exercises as short ADRs** (Architecture Decision Records). That's how you'll use this judgment at
  work.

Let's begin where Rust began: with a question the industry kept answering badly.
