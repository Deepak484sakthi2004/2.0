# Chapter 2.2 — Cargo.toml, Dependencies, Features, and Build Profiles

> **Where this sits:** Part II · Rust From First Principles · chapter 2 of 7
> **Prerequisites:** Chapter 2.1.
> **After this chapter you can:** read and write a production `Cargo.toml`; explain how semver requirements, features,
> and profiles become rustc flags; predict what a profile setting does to the generated machine code; and choose
> profile settings for a service deliberately rather than by copy-paste.

---

## Pass 1 · User level — *What does the manifest control?*

### 1. Problem

A Java service's behavior is largely decided **at launch**: `-Xmx`, `-XX:+UseZGC`, `-ea` to enable assertions,
`-Dfeature.x=true`. Change a flag, restart, and the same JAR behaves differently.

In Rust almost all of those decisions are made **at build time** and baked into the artifact: how aggressively code is
optimized, whether assertions and overflow checks even exist in the binary, what a panic does, and which optional code
is compiled at all. `Cargo.toml` isn't just packaging metadata. **It's part of your program's semantics.** Two binaries
built from the same source with different profiles can compute different answers for the same input, as §3 shows.

### 2. Mental model

```text
 Cargo.toml section     decides                                          becomes (roughly)
 ─────────────────────  ───────────────────────────────────────────────  ──────────────────────────────────
 [package]              identity, edition, MSRV (rust-version)           --edition=2024, metadata hash
 [dependencies]         WHICH CODE enters the build                      Cargo.lock + --extern flags
 [features]             WHICH OPTIONAL CODE is compiled                  --cfg feature="..."
 [profile.dev/release]  HOW it is compiled: optimization, checks,        -C opt-level, -C debug-assertions,
                        debug info, panic strategy, LTO                  -C overflow-checks, -C panic, -C lto
 [lints]                which warnings are errors                        -D / -W lint flags
 [workspace]            how packages share a lockfile and target dir     (cargo-level only)
```

### 3. Rust code

A realistic, annotated manifest for a service:

```toml
[package]
name = "gateway"
version = "0.4.0"
edition = "2024"
rust-version = "1.85"          # MSRV: the oldest compiler this package promises to build with

[dependencies]
tokio = { version = "1.47", features = ["rt-multi-thread", "net", "macros", "time"] }
serde = { version = "1", features = ["derive"] }
tracing = "0.1"                # "0.1" means >=0.1.0, <0.2.0 (in 0.x, the MINOR is the breaking part)
metrics = { version = "0.24", optional = true }   # only compiled when a feature asks for it

[dev-dependencies]             # tests, examples, benches only; never in the shipped binary
proptest = "1"

[features]
default = []
telemetry = ["dep:metrics"]    # enabling `telemetry` pulls in the optional `metrics` dependency

[profile.release]
lto = "thin"
codegen-units = 1
debug = "line-tables-only"     # keep file:line for production backtraces and profilers

[profile.release.package.billing]
overflow-checks = true         # money code panics on overflow even in release (per-package override)

[lints.rust]
unsafe_code = "forbid"
```

(Dependency version numbers are illustrative; check crates.io for current releases.)

**Profiles change behavior, not just speed.** Here's a discount function that relies on `debug_assert!`:

```rust
fn apply_discount(price_cents: u64, pct: u64) -> u64 {
    debug_assert!(pct <= 100, "discount over 100%: {pct}");
    price_cents - price_cents * pct / 100
}

fn main() {
    println!("debug_assertions = {}", cfg!(debug_assertions));
    println!("charged: {} cents", apply_discount(10_000, 150));
}
```

In a **debug** build (`dev` profile):

```text
debug_assertions = true
thread 'main' (15) panicked at src/main.rs:4:5:
discount over 100%: 150
```

In a **release** build, the same source:

```text
debug_assertions = false
charged: 18446744073709546616 cents
```

In release, the `debug_assert!` doesn't exist. It was compiled out. The overflow check on `10_000 - 15_000` doesn't
exist either (release defaults to `overflow-checks = false`). So the `u64` subtraction wraps around to about
1.8 × 10¹⁹. Neither build is "wrong" by the language's rules (Chapter 1.3: overflow is never UB). **The profile decided
which program you shipped.** §10 turns this into an incident.

---

## Pass 2 · Systems level — *How does configuration become code?*

### 4. Under the hood

**Profiles become `-C` flags.** The two built-in profiles, as documented by Cargo:

| Setting | `dev` default | `release` default | rustc flag |
|---|---|---|---|
| `opt-level` | `0` | `3` | `-C opt-level` |
| `debug` | `true` (full) | `false` | `-C debuginfo` |
| `debug-assertions` | `true` | `false` | `-C debug-assertions` |
| `overflow-checks` | `true` | `false` | `-C overflow-checks` |
| `lto` | `false` | `false` | `-C lto` |
| `panic` | `"unwind"` | `"unwind"` | `-C panic` |
| `incremental` | `true` | `false` | `-C incremental` |
| `codegen-units` | `256` | `16` | `-C codegen-units` |
| `strip` | `"none"` | `"none"`* | `-C strip` |

\* [VERSION] Since Cargo 1.77, when `debug` is off, Cargo strips debug info by default, so the precompiled std's debug
info doesn't bloat release binaries. Note that `lto = false` doesn't mean "no LTO": it means *thin local* LTO across
one crate's codegen units. `lto = "off"` disables it completely.

**What one profile setting does to machine code.** The simplest function possible:

```rust,ignore
#[inline(never)]
pub fn add_one(x: u32) -> u32 {
    x + 1
}
```

**Release** (x86-64, rustc 1.98.1):

```text
playground::add_one:
	lea	eax, [rdi + 1]        ; eax = x + 1  (lea: address arithmetic reused as a cheap add)
	ret
```

**Debug:**

```text
playground::add_one:
	push	rax                   ; set up a stack frame
	mov	eax, edi
	mov	dword ptr [rsp + 4], eax  ; spill x to the stack (so a debugger can show it)
	mov	edi, eax
	add	edi, 1                ; x + 1
	mov	dword ptr [rsp], edi  ; spill the result
	cmp	edi, eax              ; overflow check: for unsigned add, result < x means it wrapped
	jb	.LBB0_2               ;   → jump to the panic path
	mov	eax, dword ptr [rsp]
	pop	rcx
	ret
.LBB0_2:
	lea	rdi, [rip + .Lanon...1]   ; a static Location { file: "src/lib.rs", line: 5, col: 5 }
	call	qword ptr [rip + core::panicking::panic_const::panic_const_add_overflow@GOTPCREL]
```

That's two instructions against eleven, plus a cold panic path. Three different profile settings are visible in the
debug version: `opt-level = 0` (the stack spills), `overflow-checks = true` (the `cmp`/`jb` and the panic call), and
`debug` info (the spills exist so a debugger can find `x`). Notice the panic location, too. The file, line, and column
are **static data compiled into the binary**. A panic message's `src/main.rs:4:5` isn't found at run time by walking
debug info. It was embedded at compile time.

**Features become `cfg` flags.** Enabling feature `telemetry` makes cargo pass `--cfg feature="telemetry"` to rustc.
Source code checks it with `#[cfg(feature = "telemetry")]` (on items, statements, and expressions) or `cfg!(...)` (a
boolean constant). [LANG] Code under a false `#[cfg]` is **removed during macro expansion, before name resolution and
type checking**. It has to *parse*, but nothing else is checked. That's powerful (platform-specific code can mention
platform-specific APIs), and it's also dangerous, as this compiles:

```rust
// Compiles, even though `record_latency` calls a function that does not exist:
// code under a false #[cfg] is parsed, then removed before name resolution and type checking.
#[cfg(feature = "metrics")]
fn record_latency(ms: u64) {
    metrics_backend::histogram("latency_ms", ms);
}

fn handle_request() -> u64 {
    let ms = 42;
    #[cfg(feature = "metrics")]
    record_latency(ms);
    ms
}

fn main() {
    println!("handled in {} ms", handle_request());
}
```

rustc 1.98 compiles it and warns:

```text
warning: unexpected `cfg` condition value: `metrics`
 --> src/main.rs:4:7
  |
4 | #[cfg(feature = "metrics")]
  |       ^^^^^^^^^^^^^^^^^^^ help: remove the condition
  |
  = note: no expected values for `feature`
  = help: consider adding `metrics` as a feature in `Cargo.toml`
  = note: `#[warn(unexpected_cfgs)]` on by default
```

[VERSION] Since Rust 1.80, cargo tells rustc which features *exist* (`--check-cfg`), so a `cfg` naming an undeclared
feature, such as a typo, is flagged. It's a warning, not an error. The debugging exercise shows what happens when a team
ignores it.

**Dependency resolution.** A requirement like `serde = "1"` is a **caret** requirement: `>=1.0.0, <2.0.0`. For `0.x`
crates the *minor* version is treated as breaking: `"0.24"` means `>=0.24.0, <0.25.0`. Cargo picks **one version per
semver-compatible range** for the whole build, the highest that satisfies every requirement, and records it with a
checksum in `Cargo.lock`. **Features are unified the same way.** If any crate in the graph enables `tokio/net`, the
single compiled `tokio` has `net` on for everyone. That's why features must be **additive**: turning one on may only
add capabilities, never remove or change them. [VERSION] Cargo's resolver version 2 (the default since edition 2021)
keeps features of build-time and platform-specific dependencies separate. Resolver 3 (default in edition 2024) adds
MSRV-aware version selection.

**Build scripts.** `build.rs` is compiled and run before your crate. It talks to cargo through lines printed to stdout:
`cargo::rustc-cfg=has_avx2`, `cargo::rustc-link-lib=ssl`, `cargo::rerun-if-changed=proto/`. It can generate code into
`OUT_DIR` (protobuf and gRPC bindings are the classic case). A build script is arbitrary code running on your build
machine, and that's a **supply-chain surface**: review dependencies with build scripts with extra care.

### 5. Memory

What ends up in a release binary, and which settings remove what:

```text
 ┌──────────────────────────────────────────────┐
 │ your code + monomorphized generics           │ ← opt-level (size vs speed), codegen-units + lto (dead-code
 │                                              │   elimination and inlining ACROSS units and crates)
 ├──────────────────────────────────────────────┤
 │ the parts of std you actually use            │ ← lto can strip more of std; opt-level "s"/"z" favor size
 ├──────────────────────────────────────────────┤
 │ panic machinery: landing pads (cleanup code) │ ← panic = "abort" removes the landing pads
 │ + unwind tables (.eh_frame)                  │   (tables may remain for backtraces)
 ├──────────────────────────────────────────────┤
 │ static data: strings, panic Locations, ...   │
 ├──────────────────────────────────────────────┤
 │ symbol table                                 │ ← strip = "symbols"
 ├──────────────────────────────────────────────┤
 │ debug info (DWARF): often the LARGEST part   │ ← debug = false / "line-tables-only"; strip = "debuginfo";
 │                                              │   split-debuginfo moves it into a separate file
 └──────────────────────────────────────────────┘
```

Debug info is often most of an unstripped binary's size. It does nothing at run time (it isn't loaded into memory
unless something reads it), but it costs disk, image-pull time, and build time. The production pattern is to **build
with line tables, split them off, and ship them to your symbol server** rather than into the container image.

### 6. CPU / OS

- **`opt-level`** controls LLVM's pass pipeline: inlining thresholds, loop unrolling, vectorization, instruction
  selection. `3` optimizes for speed, `"s"`/`"z"` for size (less inlining and unrolling). Benchmark 2 against 3 for your
  workload rather than assuming 3 wins; more inlining can overflow the instruction cache.
- **`codegen-units`**: rustc splits a crate into N units that LLVM optimizes **in parallel**, which is faster to build.
  But LLVM can't inline across units except through local LTO. `codegen-units = 1` gives LLVM the whole crate at once:
  slower builds, often faster code.
- **`lto`**: link-time optimization lets LLVM see *across crates*. It inlines your dependencies' non-generic functions
  into your hot paths and removes code nothing reaches. `"thin"` is parallel and scalable; `"fat"` is one giant module,
  slowest to build and sometimes marginally faster to run.
- **Debug builds are slow in Rust specifically**, much more than a typical C debug build, because idiomatic Rust leans
  on inlining: iterator chains, `Option` combinators, and small trait methods are all separate calls at `opt-level = 0`,
  each with the stack spills shown above. The common remedy in development is to optimize *dependencies* while keeping
  your own crate debuggable:

```toml
[profile.dev.package."*"]
opt-level = 2        # dependencies optimized; your crate still builds fast and debugs well
```

- **Beyond profiles:** profile-guided optimization (PGO, via `-C profile-generate` / `-C profile-use`) and post-link
  layout optimization (BOLT) feed real execution profiles back into code layout and inlining decisions. They're covered
  in Part XX, with measurements.

---

## Pass 3 · Architect level — *Which settings for which system?*

### 7. Trade-offs

**The profile decision matrix for a production service.** None of these has a universally right value. Each depends
on the system:

| Setting | Options | You gain | You pay | Typical service choice |
|---|---|---|---|---|
| `opt-level` | 2 / 3 / s / z | Speed (2, 3) or size (s, z) | Build time; I-cache pressure at 3 | 3, but benchmark against 2 |
| `lto` | off / false / thin / fat | Cross-crate inlining, smaller binary | Build and link time (fat: a lot) | `"thin"` |
| `codegen-units` | 1 … 256 | Better optimization at 1 | Build parallelism | 1 for release artifacts |
| `debug` | false / line-tables-only / full | Symbolized backtraces and profiles | Size, build time | `"line-tables-only"`, split off |
| `panic` | unwind / abort | *Unwind:* per-task isolation, destructors run. *Abort:* smaller, simpler, fail-fast. | *Unwind:* landing pads. *Abort:* one bug kills every in-flight request. | Depends on the system (§9) |
| `overflow-checks` | true / false | Arithmetic bugs become panics | A branch per operation (often cheap) | `true` for money and quota code |
| `incremental` | true / false | Fast rebuilds | Less optimized code | Dev only |

**Feature design rules**, which follow from unification:

1. Features must be **additive**. Mutually exclusive features ("`backend-postgres` XOR `backend-mysql`") break as soon
   as two crates in one build choose differently. Unification turns both on.
2. **Library default features should be minimal.** Downstream crates can add features but can't reliably remove yours:
   if *any* dependent leaves your defaults on, they're on for everyone.
3. Removing a feature, or making a default feature non-default, is a **breaking change** for dependents.
4. CI must build **every feature combination that matters**. Disabled-`cfg` code isn't type-checked (§4). Tools like
   `cargo hack --each-feature` exist for this.

**Dependency policy.** Every dependency is code you ship and trust, including its build script and its `unsafe`. The
checks worth automating: `cargo tree -d` (duplicate versions), `cargo tree -e features` (why is this feature on?),
`cargo deny` (licenses, advisories, bans, duplicates), `cargo audit` (the RustSec advisory database), and `cargo vet`
(recorded human review).

### 8. Java comparison

| Java | Rust | Where the analogy breaks |
|---|---|---|
| JVM flags at launch (`-Xmx`, `-XX:...`) | Profile settings at **build** time | A Rust binary can't be re-tuned by restarting it with different flags. There's no `-Xmx` at all, because there's no managed heap to size. |
| `assert` + `-ea` | `debug_assert!` (dev only) and `assert!` (always) | Java assertions can be toggled **per class at run time**. `debug_assert!` is **removed at compile time**, and no run-time switch exists. |
| JIT optimization tiers | `opt-level` | The JIT optimizes the **hot** code it observes. `opt-level` applies uniformly (PGO narrows the gap). |
| Maven profiles | Cargo profiles *and* features | Maven profiles reconfigure the build. Cargo **features change which source code is compiled**. |
| `<optional>true</optional>` dependency | `optional = true` + `dep:` feature | In Cargo, optional features are **unified across the whole graph**. |

> **Analogy limit.** "`debug_assert!` is Java's `assert`" holds for intent: a check you don't want in production. It
> fails operationally. A Java team can turn assertions back on in production to chase a bug (`-ea` on a canary). A Rust
> team can't; the check isn't in the binary. If you'd ever want the check in production, it should be `assert!`, or
> real error handling, from the start.

### 9. Production scenario

**Meridian chooses release profiles per system**, and the *panic strategy* differs across them for good reasons:

| System | `panic` | Why |
|---|---|---|
| **API gateway** (Tokio, thousands of concurrent requests per pod) | `unwind` | [LIB] Tokio catches a panic inside a spawned task and reports it as a `JoinError`. One buggy request fails; the other 5,000 in flight don't. With `abort`, a single bad request would kill every connection on the pod. |
| **Fraud feature library** (a `cdylib` loaded into the JVM via FFM) | `unwind` *plus* `catch_unwind` at every exported function | A Rust panic must never unwind into JVM frames. [VERSION] Since Rust 1.81, a panic unwinding out of an `extern "C"` function aborts the process, which here means the *JVM*. So every entry point catches panics and converts them into error codes (Part XVI). With `panic = "abort"`, `catch_unwind` has nothing to catch, and any panic would kill the JVM. |
| **`logstat` CLI** (Project Level 1) | `abort` | A panic is a bug; exiting immediately with a message is fine, and the binary is smaller. |
| **Billing crate** (inside the gateway) | — | `[profile.release.package.billing] overflow-checks = true`. Money arithmetic that overflows must stop, not wrap. [VERSION] Per-package overrides can set `overflow-checks`, `opt-level`, `debug-assertions`, and similar settings, but **not** `panic` or `lto`, which are whole-program decisions. |

The gateway's release profile is `opt-level = 3`, `lto = "thin"`, `codegen-units = 1`, `debug = "line-tables-only"`
with split debug info uploaded to the symbol store. Every choice has a written reason and a benchmark attached (Part
XX).

### 10. Failure scenario

**The promotion that charged 18 quintillion cents.** Meridian's checkout service computes discounts with the
`apply_discount` function from §3. The only validation of `pct` is the `debug_assert!`. A test suite covers it:

```rust,ignore
#[test]
#[should_panic(expected = "discount over 100%")]
fn rejects_discount_over_100() {
    apply_discount(10_000, 150);
}
```

It passes, because `cargo test` uses the `dev` profile, where the assertion exists. A marketing tool then starts sending
discounts in **basis points** instead of percent (1500 for 15%). In production (release profile): no assertion, no
overflow check, `u64` wraparound, and a charge of `18446744073709546616` cents goes to the payment processor. The
processor rejects it as "amount too large," so every discounted checkout fails for the length of the campaign. At least
nothing was charged. With a different formula, the result could have wrapped to a plausible-looking wrong amount.

The layered fixes:

1. **Validation that exists in every build:** return `Result` for invalid input (Part VIII), or use `assert!`, not
   `debug_assert!`.
2. **Checked arithmetic in money code:** `checked_sub` / `checked_mul`, or `overflow-checks = true` for the billing
   package.
3. **Types that carry units:** `Percent(u8)` vs `BasisPoints(u16)` as distinct newtypes (Part V), so the tool's change
   is a compile error.
4. **Run the test suite in the release profile too** (`cargo test --release`). This `#[should_panic]` test fails there,
   and that failure would have exposed the gap.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part II).*

1. Why must Cargo features be additive? What goes wrong with mutually exclusive features, concretely?
2. `debug_assert!` vs `assert!`: when is each appropriate in production code?
3. Walk through what `lto = "thin"` plus `codegen-units = 1` changes in the pipeline and in the binary. What does it
   cost?
4. Choose `panic = "unwind"` or `"abort"` for (a) a multi-tenant Tokio gateway, (b) a library loaded into a JVM, (c) a
   CLI. Justify each.
5. Why doesn't code under a disabled `#[cfg(feature = ...)]` need to type-check? What does that mean for CI?
6. Explain caret requirements for `1.x` and `0.x` crates. Why does `0.x` treat a minor bump as breaking?
7. Your release binary is 40 MB. Where do you look first, and which settings would you change, in what order?
8. Why might you enable `overflow-checks` in release for only one package, and how would you do it?

### 12. Exercises

- **Beginner.** Run the discount listing in debug and release (the Playground has a mode selector). Then change it so
  both builds reject the input identically, without using `debug_assert!`.
- **Intermediate.** Write the `Cargo.toml` for a library with an optional `serde` integration: an optional dependency,
  a `serde` feature using `dep:`, and `#[cfg_attr(feature = "serde", derive(serde::Serialize))]` on a struct. Why is
  `cfg_attr` better than duplicating the struct?
- **Advanced.** Build a small program (for example an HTTP client with a couple of dependencies) with each of:
  default release; `strip = true`; `opt-level = "z"`; `lto = "fat"` + `codegen-units = 1`; `panic = "abort"`. Record
  binary size and build time in a table. Which setting gave the most for the least?
- **Systems.** In Compiler Explorer, compile `add_one` with `-C opt-level=0`, `1`, `2`, `3`, then with and without
  `-C overflow-checks`. Explain every instruction that appears or disappears.
- **Architecture.** Write your organization's **standard release profile** for services, with one sentence of
  justification per setting and the benchmark you'd use to revisit each.

### 13. Debugging exercise

The listing in §4 (`record_latency` calling a non-existent `metrics_backend`) has been in Meridian's codebase for eight
months. CI is green. The observability team finally enables the `metrics` feature in their build, and it fails with
`unresolved module metrics_backend` and a dozen other errors.

1. How did broken code pass CI for eight months? Name the compiler phase that removed it.
2. What were the two `unexpected_cfgs` warnings telling the team the whole time?
3. Propose two CI changes that would have caught this, and one code-review rule.

### 14. Design exercise

**Feature design for Meridian's internal HTTP client library**, used by 30 services. It must support:

- a blocking API (for CLIs) and an async API (for Tokio services);
- two TLS backends: `rustls` (the default) and the platform's native TLS (required by one team for FIPS-validated
  crypto);
- optional `serde` JSON helpers and optional `tracing` spans.

Design the `[features]` table and the default features. Explain how you keep every feature additive, especially for
the two TLS backends, which *sound* mutually exclusive. What happens when two services in one workspace choose different
TLS backends? Decide what the library should do at run time if both are compiled in, and justify it.
