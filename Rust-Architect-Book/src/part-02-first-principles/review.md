# Part II Review — Architecture Review & Interview Mode

> Consolidate Part II, then use it: review a pull request the way a principal engineer would, and answer senior-level
> questions without notes. Answers are in **Appendix A, Part II**.

---

## Part II on one page

```text
 FILES ON DISK                 Cargo.toml ─── Cargo.lock ─── rust-toolchain.toml
      │                        [dependencies] which code · [features] which optional code (additive, unified)
      │                        [profile.*] HOW it compiles: opt-level, lto, codegen-units, panic,
      │                                     debug-assertions, overflow-checks, debug info
      ▼
 cargo ──► rustc, once per CRATE (the unit of compilation, privacy, coherence)
      │     .rlib = object code + metadata (+ MIR of generic/inline fns, so YOUR crate instantiates them)
      ▼
 SOURCE CONSTRUCTS             what the compiler makes of them
   let x = 10;                 a NAME; storage is the compiler's choice: gone (constant), a register,
                               or a debug-only stack slot. MIR → SSA LLVM IR → registers
   mut / shadowing             same SSA either way; for humans and the borrow checker, not for speed
   as / From / TryFrom         `as` never fails (truncates, wraps, saturates); TryFrom at boundaries
   expressions, (), !          blocks have values; ; discards; () is a real type; ! fits anywhere
   tuples, [T; N], &[T]        inline values; &[T] = pointer + length; big returns go via a hidden pointer
   match                       EXHAUSTIVE (usefulness algorithm, witnesses like `u8::MAX`);
                               compiles to lookup tables, jump tables, or compare chains
   struct / enum / impl        AND / OR (a tagged union; niches make Option free); receivers = ownership modes
   mod / pub / pub(crate)      privacy flows DOWN the module tree; private fields make `new` the only door;
                               privacy is what lets unsafe code trust its own invariants
      ▼
 BINARY                        statically linked Rust code, a target-cpu baseline, panic strategy baked in
```

## Ten ideas to carry forward

1. **Configuration is semantics.** Profiles decide whether assertions and overflow checks exist, and what a panic does.
   The same source can ship as two different programs.
2. **A crate isn't a JAR.** Generic code from a dependency is compiled *in your crate*, and incompatible major versions
   can coexist.
3. **Build flags describe the fleet, not the build machine** (`target-cpu`).
4. **A binding is a name, not a box.** Storage is a compilation decision, and borrowing is what forces a value to have
   an address.
5. **`as` never fails.** That's the problem. Narrow with `TryFrom` at every boundary.
6. **Everything is an expression**, and `()` and `!` are ordinary members of the type system.
7. **Exhaustiveness is a proof you can lean on**, until a wildcard opts you out of future variants. Wildcards should
   fail closed.
8. **Enums are tagged unions:** the largest variant sets everyone's size, and niches can make the tag free.
9. **Receivers (`&self`, `&mut self`, `self`) are the first ownership contracts** you write.
10. **Privacy is the invariant boundary**, including memory-safety invariants, and the crate graph is an architecture
    you can enforce with the build.

---

## Architecture review: the quota-service pull request

A Meridian team opens a PR for its first Rust service, a per-tenant quota enforcer. You're the reviewer. **Find every
problem you can**, classify each one (correctness, safety, operability, performance, maintainability), and propose a
fix. There are at least twelve issues, and each maps to something in Part II.

```toml
# Cargo.toml
[package]
name = "quota-service"
version = "0.1.0"
edition = "2024"

[features]
default = ["redis-backend"]
redis-backend = ["dep:redis"]
memory-backend = []            # choose exactly ONE backend

[dependencies]
redis = { version = "0.27", optional = true }
tokio = { version = "1", features = ["full"] }

[profile.release]
panic = "abort"
debug = false
```

```toml
# .cargo/config.toml
[build]
rustflags = ["-C", "target-cpu=native"]   # 8% faster in the benchmark on the CI runner
```

```rust,ignore
// src/lib.rs
pub mod quota {
    pub struct Quota {
        pub used: u32,
        pub limit: u32,
    }

    impl Quota {
        pub fn consume(&mut self, n: u64) {
            debug_assert!(self.used as u64 + n <= self.limit as u64, "over quota");
            self.used += n as u32;
        }
    }

    pub enum Command {
        Consume { tenant: String, n: u64 },
        Reset { tenant: String },
        Snapshot([u8; 65536]),
    }

    pub fn validate(cmd: &Command) -> Result<(), String> {
        match cmd {
            Command::Consume { n, .. } if *n == 0 => Err("empty consume".to_string()),
            _ => Ok(()), // other commands don't need validation
        }
    }
}
```

Write your review as a list of comments, each with: **location → problem → consequence in production → fix.** Then
write a two-line summary verdict: approve, approve with changes, or request changes, and why.

---

## Interview mode

*Senior-level. Answer aloud or in writing, without notes, before checking Appendix A.*

### Language

1. Does `let x = 10;` guarantee that `x` has storage at run time? What's the difference between a Rust binding and a
   JVM local variable?
2. Why are `()` and `!` types? What would break in the language without each?
3. Why is every `match` exhaustive? How do guards and wildcards interact with that guarantee?
4. What do the receivers `&self`, `&mut self`, and `self` communicate to callers and to the compiler?

### Compiler

5. What does cargo pass to rustc, and what does an `.rlib` contain that a JAR doesn't?
6. What does the MIR for `x + 1` look like in a debug build, and where does the overflow check come from?
7. When does a `match` compile to a lookup table, a jump table, or a comparison chain?
8. Why can fixing the only error rustc reported reveal new errors that were there all along?

### Performance

9. Why are Rust debug builds disproportionately slow, and what's the standard mitigation?
10. What do `lto = "thin"` and `codegen-units = 1` buy, and what do they cost?
11. What makes an enum large? Why does it matter for `Vec`s and channels, and how do you fix it?

### Architecture

12. How do you choose a panic strategy (`unwind` vs `abort`) per system? Give three systems with different answers.
13. How does a Cargo workspace enforce layering, and what architectural rules *can't* it enforce?
14. Why is every `pub` item a semver promise? What's a public dependency?
15. "Build flags describe the fleet, not the build machine." Explain with an incident.

---

## Looking ahead: Part III

Part II kept circling one idea without naming it: **who owns a value, and who may use it, and when.** You saw it in
`self` receivers, in `Record<'a>` borrowing from a line buffer, in `Summary` copying out only what must outlive the line,
and in arrays and `Vec`s being moved. Part III makes it the subject: ownership, moves, `Copy` and `Clone`, borrowing,
slices, `String` vs `&str`, `Drop`, and how to model graphs when ownership has to form a tree. It ends with **Project
Level 2**, a streaming file processor.
