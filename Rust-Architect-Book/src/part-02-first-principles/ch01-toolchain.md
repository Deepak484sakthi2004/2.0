# Chapter 2.1 — The Toolchain: rustup, rustc, cargo, and What a Crate Is

> **Where this sits:** Part II · Rust From First Principles · chapter 1 of 7
> **Prerequisites:** Part I.
> **After this chapter you can:** explain what each tool in the Rust toolchain does, what `cargo build` actually
> runs, what a crate is (and why it isn't a JAR), how editions and target triples work, and how to set up a toolchain
> on a machine with very little disk.

---

## Pass 1 · User level — *What are the pieces, and how do I use them?*

### 1. Problem

In Java, the path from source to running program goes through the JDK (`javac`, `java`), a build tool (Maven or
Gradle), an artifact format (JARs), a runtime lookup mechanism (the classpath or module path), and a toolchain manager
(SDKMAN, Gradle toolchains). You've debugged every one of those layers: dependency mediation picking the wrong version,
`NoSuchMethodError` at run time, "works on my JDK."

Rust has an equivalent stack, but the responsibilities are split differently, and some Java problems simply don't
exist in it, because nothing is looked up at run time. To reason about builds, CI, reproducibility, and deployment you
need to know exactly which tool decides what.

### 2. Mental model

```text
 rustup    the TOOLCHAIN MANAGER
   │       installs and selects toolchains (stable / beta / nightly / a pinned version),
   │       per-directory overrides, extra targets, and components
   ▼
 a toolchain = rustc + cargo + std (precompiled for each target) + optional clippy, rustfmt, docs
   │
 cargo     the BUILD SYSTEM and PACKAGE MANAGER
   │       reads Cargo.toml, resolves and locks dependencies (Cargo.lock), downloads sources,
   │       runs build scripts, then invokes rustc ONCE PER CRATE in dependency order
   ▼
 rustc     the COMPILER
           compiles exactly ONE crate per invocation → a library (.rlib) or an executable
```

The vocabulary is precise and worth getting right:

| Term | Meaning | Closest Java analog | Where the analogy breaks |
|---|---|---|---|
| **Crate** | One compilation unit: the tree of modules rustc compiles in one invocation. A *library* crate or a *binary* crate. | A JAR's worth of classes | A crate is also the unit of **privacy** (`pub(crate)`), **coherence** (Part VI), and incremental compilation. Generic code from a library crate is compiled into the *user's* crate. |
| **Package** | A `Cargo.toml` plus the crates it builds: at most one library, any number of binaries, examples, tests, benches | A Maven module / Gradle project | — |
| **Module** | A namespace *inside* a crate (`mod billing;`) | A Java package | Modules nest, and child modules see their parent's private items (Chapter 2.7). |
| **Workspace** | Several packages sharing one `Cargo.lock` and one `target/` | A multi-module Maven build | — |
| **crates.io** | The public registry | Maven Central | Published versions are immutable; they can be *yanked*, never deleted. |
| **Edition** | A per-crate language dialect (2015, 2018, 2021, 2024) | Nothing close (not `--release 21`) | Crates of different editions link together freely (§4). |
| **Target triple** | The platform you compile *for*: `x86_64-unknown-linux-gnu` | "The JVM" | There's no portable bytecode; each target is a separate build. |

### 3. Rust code

A new package:

```text
$ cargo new logstat
    Creating binary (application) `logstat` package

logstat/
├── Cargo.toml          # the manifest: name, version, edition, dependencies, profiles
├── .gitignore          # ignores /target
└── src/
    └── main.rs         # the binary crate's root
```

The conventional layout, which cargo discovers without configuration:

```text
src/main.rs          binary crate root (crate name = package name)
src/lib.rs           library crate root; main.rs can use it as `logstat::...`
src/bin/*.rs         additional binaries
tests/*.rs           integration tests: EACH FILE IS A SEPARATE CRATE that uses your library's public API
examples/*.rs        runnable examples (`cargo run --example x`)
benches/*.rs         benchmarks
build.rs             build script: compiled and run BEFORE the package (codegen, linking native libs)
```

Most of what a program knows about its build is **decided at compile time**. This listing prints some of it (verified
in a debug build and a release build):

```rust
use std::env::consts;
use std::mem::size_of;

fn main() {
    // Baked in at COMPILE time, by cargo (env!) and by rustc (cfg!, consts):
    println!("package:          {} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    println!("debug_assertions: {}", cfg!(debug_assertions));
    println!("target os/arch:   {}/{}", consts::OS, consts::ARCH);
    println!("pointer width:    {} bits", size_of::<usize>() * 8);
    // Read at RUN time, from the process environment:
    println!("RUST_LOG now:     {:?}", std::env::var("RUST_LOG").ok());
}
```

```text
package:          playground 0.0.1          (both builds)
debug_assertions: true                      (debug)    /  false (release)
target os/arch:   linux/x86_64
pointer width:    64 bits
RUST_LOG now:     None
```

`env!("CARGO_PKG_NAME")` isn't a function call. It's a macro that cargo's environment turns into a string literal
**inside the binary**. `cfg!(debug_assertions)` becomes a constant `true` or `false`, and the optimizer deletes whatever
code it guards. Only the last line reads anything at run time. A Java program asks the JVM at run time
(`System.getProperty("os.arch")`). A Rust program was compiled for one target, and it knows it.

---

## Pass 2 · Systems level — *What does `cargo build` actually do?*

### 4. Under the hood

**`cargo build`, step by step:**

```text
1. Read Cargo.toml (and the workspace root's, if any).
2. Resolve dependencies:  semver requirements → concrete versions → written to Cargo.lock
                          (or read from Cargo.lock if it exists and still satisfies the manifest)
3. Fetch sources          → ~/.cargo/registry  (checksums verified against Cargo.lock)
4. Plan a unit graph:     one "unit" per (crate, target, profile, feature set)
5. For each unit, in dependency order, in parallel where the graph allows:
     a. compile and run its build script (build.rs), if any
     b. invoke rustc for the crate
6. Fingerprint every unit, so the next build recompiles only what changed.
```

`cargo build -v` prints every rustc invocation. A representative one (trimmed; run it on your own project to see yours):

```text
rustc --crate-name logstat --edition=2024 src/main.rs --crate-type bin
      --emit=dep-info,link -C debuginfo=2 -C metadata=<hash> -C extra-filename=-<hash>
      --out-dir target/debug/deps -C incremental=target/debug/incremental
      -L dependency=target/debug/deps --extern <dep>=target/debug/deps/lib<dep>-<hash>.rlib
```

Each flag is a decision cargo made for you: which edition, which profile settings (`-C debuginfo=2` is the `dev`
profile), where dependencies are (`--extern`), and a **metadata hash** that makes symbol names unique, which is part of
how two versions of the same crate can coexist (§7).

**What's inside an `.rlib`?** [RUSTC] It's an archive holding the crate's compiled object code **plus metadata**:
the crate's types, its public items, and the **MIR of its generic and `#[inline]` functions**. That last part is the
key difference from a JAR:

```text
 serde's .rlib                                     your crate
┌──────────────────────────────────┐             ┌────────────────────────────────────────────┐
│ object code for non-generic fns  │   link ──►  │                                            │
│ metadata:                        │             │ fn to_json<T: Serialize>(v: &T) ...        │
│   types, signatures, traits      │             │   used with T = Order                      │
│   MIR of generic functions  ─────┼──────────► │ → rustc instantiates serde's generic code  │
│   (e.g. serialize::<T>)          │  copied in  │   FOR Order, INSIDE YOUR CRATE's codegen   │
└──────────────────────────────────┘             └────────────────────────────────────────────┘
```

A JAR ships bytecode that works for every `T`, because generics are erased. An `.rlib` can't ship machine code for a
`T` it has never seen, so it ships the *recipe*, and your crate cooks it. That's why generic-heavy dependencies make
**your** crate slower to compile (Part VII).

Metadata also enables **pipelined compilation**. [RUSTC] rustc writes a crate's metadata (`.rmeta`) before it finishes
generating code, and cargo starts compiling dependent crates as soon as the metadata exists. `cargo check` stops after
metadata entirely (no code generation), which is why it's several times faster than `cargo build` and is the right
command for the edit-compile loop.

**std is precompiled.** [RUSTC] The toolchain ships `std`, `core`, and `alloc` as `.rlib`s for each installed target
(the `rust-std` component). `rustup target add aarch64-unknown-linux-gnu` downloads another precompiled std. Rebuilding
std from source with custom flags is a nightly-only feature (`-Z build-std`).

**Editions.** [LANG] An edition is chosen **per crate** in `Cargo.toml`, and crates of different editions link
together freely, because editions only change how *source text* is interpreted. After parsing and lowering, everything
is the same compiler IR. Editions can reserve keywords, change defaults, and turn warnings into errors. They can't
change the meaning of already-compiled code or fragment the ecosystem. Every edition change is designed to be
mechanically migratable (`cargo fix --edition`).

| Edition | Rust version | Examples of what changed |
|---|---|---|
| 2015 | 1.0 | The original |
| 2018 | 1.31 | Module path overhaul (`crate::`), `async`/`await` reserved, NLL borrow checking |
| 2021 | 1.56 | Disjoint closure captures, `IntoIterator` for arrays, panic macro consistency |
| 2024 | 1.85 | `gen` reserved, if-let chains (1.88+), `unsafe extern` blocks, new temporary scopes, resolver 3 by default |

[VERSION] This book uses edition 2024 throughout. The edition matters in practice: code that compiles in a 2021 crate
can fail in a 2024 crate (see the debugging exercise).

### 5. Memory (and disk)

Where the bytes live on disk. For this book's reader, on a small SSD, this matters more than usual:

```text
~/.rustup/                                (RUSTUP_HOME: relocatable via env var)
  toolchains/stable-x86_64-pc-windows-gnu/
    bin/        rustc, cargo, (clippy, rustfmt)
    lib/rustlib/x86_64-pc-windows-gnu/lib/*.rlib      ← precompiled std for this target
    share/doc/  ← the rust-docs component: offline HTML docs, hundreds of MB; skip with the minimal profile
~/.cargo/                                 (CARGO_HOME: relocatable via env var)
  registry/     downloaded crate sources and index cache (grows across projects)
<project>/target/                         (CARGO_TARGET_DIR: can be shared between projects)
  debug/        executables, deps/*.rlib, incremental/ caches, build/ script outputs
  release/      the same for release builds
```

`target/` is usually the largest item. Debug info and incremental caches can reach gigabytes on real projects. The
levers:

```toml
# Cargo.toml: shrink dev builds (the trade-off is weaker debugger support)
[profile.dev]
debug = "line-tables-only"   # file:line info for backtraces, no variable info
```

**Toolchain setup on a disk-constrained Windows machine.** There are three realistic options:

| Option | Disk cost | Notes |
|---|---|---|
| **`x86_64-pc-windows-msvc`** (rustup's default on Windows) | Rust toolchain + **Visual Studio Build Tools and a Windows SDK (several GB)** | The best-supported Windows target; needs Microsoft's linker |
| **`x86_64-pc-windows-gnu`**, minimal profile | Rust toolchain only (a few hundred MB); ships a self-contained MinGW linker | `rustup-init --default-host x86_64-pc-windows-gnu --profile minimal`; set `RUSTUP_HOME`/`CARGO_HOME` to another drive if you have one |
| **No local toolchain** | None | The Rust Playground (enough for Parts I–VI), or a cloud dev environment (GitHub Codespaces and similar) for multi-file projects and Linux-only tools such as `perf` |

For this book, most of which assumes Linux for its OS-level chapters, a cloud Linux environment is the most useful
choice once you reach the projects. Parts XIX and XX (linkers, `perf`, flame graphs) are much easier on real Linux.

### 6. CPU / OS

**Target triples** name the platform: `<arch>-<vendor>-<os>-<env/abi>`.

| Triple | Meaning |
|---|---|
| `x86_64-unknown-linux-gnu` | 64-bit x86 Linux, glibc, dynamically linked to libc |
| `x86_64-unknown-linux-musl` | 64-bit x86 Linux, musl libc, **fully static** binary |
| `aarch64-unknown-linux-gnu` | 64-bit ARM Linux (AWS Graviton, Ampere) |
| `x86_64-pc-windows-msvc` / `-gnu` | 64-bit Windows with the MSVC or MinGW toolchain |
| `aarch64-apple-darwin` | Apple Silicon macOS |
| `wasm32-unknown-unknown` | WebAssembly with no OS |

Rust sorts targets into **tiers**: Tier 1 is "guaranteed to work" (built and fully tested on every change), Tier 2 is
"guaranteed to build," and Tier 3 is best-effort. Production services should stay on Tier 1 or well-used Tier 2
targets.

**The target triple fixes the instruction set. The *CPU* setting fixes which instructions are allowed.** The LLVM IR
that rustc 1.98 generated for this Part's listings carries this attribute:

```text
attributes #0 = { ... "probe-stack"="inline-asm" "target-cpu"="x86-64" }
```

`"target-cpu"="x86-64"` is the **baseline** x86-64 of 2003: SSE2, no AVX. The binary runs on any x86-64 machine, and
the compiler may not use newer vector instructions. (`"probe-stack"` is the stack-probing mechanism from Chapter 1.3,
visible in the IR.) You can raise the floor with `-C target-cpu=x86-64-v3` (AVX2, BMI2, and so on: roughly Haswell,
2013, and later) or `-C target-cpu=native` (whatever the build machine has). §10 shows why `native` is dangerous.

**Linking.** [RUSTC] Rust links its own crates, std included, **statically** into the executable. On
`*-linux-gnu` targets it still links **dynamically** to the system glibc. On `*-linux-musl` targets the binary is fully
static, a single file that runs on any Linux kernel with no libc dependency. That's ideal for `FROM scratch` container
images. [LIB] musl's `malloc` is notably slower under multithreaded load, so static musl services usually swap in
another allocator (mimalloc or jemalloc). Part XIX covers linking in depth.

---

## Pass 3 · Architect level — *Which build decisions matter in production?*

### 7. Trade-offs

**One crate or many?**

| | Single crate | Workspace of several crates |
|---|---|---|
| Compile parallelism | Codegen units only; the front end is largely sequential per crate [RUSTC] | Independent crates compile in parallel |
| Incremental rebuilds | Any change re-checks the whole crate | Only changed crates and their dependents rebuild |
| Architecture enforcement | Only module privacy | **The crate graph is acyclic**, so layering is enforced by the compiler (Chapter 2.7) |
| Friction | Low | More manifests; the orphan rule (Part VI) bites at crate boundaries; `pub` surfaces between crates |
| Cross-crate inlining | Everything is visible | Non-generic functions need `#[inline]` or LTO to inline across crates |

**Pinning and reproducibility.** A reproducible Rust build pins: the **toolchain** (`rust-toolchain.toml`), the
**dependencies** (a committed `Cargo.lock`; since 2023 the Cargo team recommends committing it for libraries too), the
**target and CPU flags**, and anything **build scripts** fetch (ideally nothing). `Cargo.lock` doesn't pin the
toolchain, and a pinned toolchain doesn't pin a build script that downloads things.

```toml
# rust-toolchain.toml: everyone, and CI, gets exactly this
[toolchain]
channel = "1.98.1"
profile = "minimal"
components = ["clippy", "rustfmt"]
targets = ["x86_64-unknown-linux-musl"]
```

**Stable vs nightly.** Production code uses stable. The six-week release train means stable gets new features about
every six weeks, with a strong backward-compatibility promise. Nightly is for compiler development, some tooling (Miri,
`-Z` flags), and experiments. A library should declare its minimum supported Rust version (`rust-version = "1.85"` in
`Cargo.toml`). [VERSION] Since Rust 1.84, cargo's resolver can respect that when choosing dependency versions (the
default in edition 2024).

### 8. Java comparison

| Concern | Maven / Gradle + JVM | Cargo + rustc |
|---|---|---|
| Build definition | XML / Groovy / Kotlin DSL, plugins | Declarative TOML + an optional `build.rs` (Rust code) as the escape hatch |
| Version conflict | **One version per artifact on the classpath.** Maven's "nearest wins" mediation can pick an *older* version than a library needs, so `NoSuchMethodError` at run time. Gradle picks the highest. | **One version per semver-compatible range** (the highest that satisfies everyone). **Incompatible majors coexist** in one binary. Conflicts show up as compile errors, never at run time. |
| When dependencies are checked | Partly at run time (class loading, linkage errors) | Entirely at compile and link time |
| Artifact | Bytecode JAR + a JVM to run it | A native executable for one target |
| Toolchain pinning | Gradle toolchains, `.sdkmanrc` | `rust-toolchain.toml` |
| Reflection-based plugins | Common (Spring, ServiceLoader) | None. Plugins go through the C ABI, WASM, or processes (Part XVI). |

**Two versions of one crate** is the part a Java engineer finds strangest. If your crate depends on `rand 0.8` and a
dependency depends on `rand 0.7`, both are compiled into the binary. They're *different crates* with different metadata
hashes, so `rand_0_7::Rng` and `rand_0_8::Rng` are unrelated types. Pass a value from one to an API expecting the other
and you get a confusing compile error along the lines of *expected `StdRng`, found `StdRng`*, with the note *"perhaps
two different versions of crate `rand` are being used?"*. `cargo tree -d` lists every duplicated crate. The trade-off
is deliberate. There's no classpath hell, but duplicates cost binary size and compile time, and types can't cross the
version boundary.

> **Analogy limit.** "A crate is a JAR" works for distribution: both are the unit you publish and depend on. It fails
> for compilation. A JAR is compiled *once* and its bytecode serves every caller. A crate's generic code is compiled
> *again in every crate that instantiates it*. The dependency is part of your build, not just your classpath.

### 9. Production scenario

**Meridian's first Rust service in CI.** The gateway team sets up the pipeline:

- `rust-toolchain.toml` pins `1.98.1`. Toolchain upgrades are a reviewed PR, done every second or third release, with
  the full test suite and a benchmark run.
- `Cargo.lock` is committed. Nightly CI runs `cargo update` on a branch to surface upstream changes early, and
  `cargo deny` checks licenses, advisories, and duplicate versions.
- The build image caches `~/.cargo/registry` and `target/` keyed on `Cargo.lock` and the toolchain version. Cold release
  builds take minutes; warm incremental checks take seconds.
- Targets: `x86_64-unknown-linux-musl` for a `FROM scratch` image, with mimalloc as the global allocator, and
  `aarch64-unknown-linux-musl` for the Graviton node pool.
- CPU flags: the baseline for x86-64. The team measured `x86-64-v3` separately and only adopted it once platform
  engineering confirmed the whole fleet supports AVX2 (see §10).

### 10. Failure scenario

**The SIGILL that only hit some pods.** A performance engineer adds this to the repo to speed up a benchmark:

```toml
# .cargo/config.toml
[build]
rustflags = ["-C", "target-cpu=native"]
```

CI runners are new instances with AVX-512. The release binary now contains AVX-512 instructions in a few hot loops
that LLVM auto-vectorized. Production runs a mixed node pool. On nodes whose CPUs lack AVX-512, the process dies with
**`SIGILL` (illegal instruction)**, and it dies *only* when a request reaches one of those loops. That's minutes after
startup, on about a third of pods, with a stack trace pointing into innocent JSON code.

What made it hard: the build was "the same" everywhere, tests passed (CI runners had AVX-512), and the crash depended on
both the hardware and the code path. **The fix:** remove `native`, choose an explicit fleet-wide baseline
(`x86-64-v2` or `x86-64-v3`, if every node supports it), and where wide vectors really matter, use **runtime feature
detection** (`is_x86_feature_detected!("avx512f")`) to pick a code path at run time (Part XX). The rule to take away:
**build flags describe the fleet, not the build machine.**

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part II).*

1. Define *package*, *crate*, *module*, and *workspace*. Give the closest Java analog for each and say where it breaks.
2. What's inside an `.rlib` besides object code, and why does it need to be there? Contrast with a JAR.
3. How can two major versions of the same crate end up in one binary? What problems does that cause, and how does it
   compare with Java's classpath?
4. What is an edition? Why can crates of different editions be linked together, and what kinds of change can an
   edition make?
5. Why is `-C target-cpu=native` dangerous in CI? What are the safer options?
6. What does `Cargo.lock` guarantee, and what doesn't it? List everything you'd pin for a reproducible build.
7. What is pipelined compilation, and why is `cargo check` so much faster than `cargo build`?
8. When would you ship a `*-linux-musl` static binary instead of a `*-linux-gnu` one? What's the catch?

### 12. Exercises

- **Beginner.** Create a package with `cargo new`, build it in debug and release, and list everything under `target/`.
  Run `cargo build -v` and explain each rustc flag.
- **Intermediate.** Turn it into a library plus a binary: move the logic to `src/lib.rs`, call it from `src/main.rs`,
  and add an integration test in `tests/`. Why can the integration test only use your crate's *public* API?
- **Advanced.** Clone a real-world project (ripgrep is a good choice) and run `cargo tree -d`. Find a duplicated crate
  and explain, from the dependency requirements, why cargo couldn't unify it.
- **Systems.** On Linux or WSL, build the same program for `x86_64-unknown-linux-gnu` and `x86_64-unknown-linux-musl`.
  Compare `ldd` output, file size, and startup time. Then check the dynamic dependencies with `readelf -d`.
- **Architecture.** Write a one-page **toolchain policy** for a Rust team: pinning, upgrade cadence, MSRV for internal
  libraries, allowed targets and CPU baselines, lockfile rules, and supply-chain checks.

### 13. Debugging exercise

A colleague's snippet compiles in their crate but fails when you paste it into yours:

```rust,compile_fail
fn main() {
    let gen = 5;
    println!("{gen}");
}
```

Your crate (edition 2024) reports:

```text
error: expected identifier, found reserved keyword `gen`
 --> src/main.rs:4:9
  |
4 |     let gen = 5;
  |         ^^^ expected identifier, found reserved keyword
  |
help: escape `gen` to use it as an identifier
  |
4 |     let r#gen = 5;
  |         ++

error[E0425]: cannot find value `r#gen` in this scope
 --> src/main.rs:5:16
  |
5 |     println!("{gen}");
  |                ^^^ not found in this scope
```

1. Why does it compile in your colleague's crate? What single line in their `Cargo.toml` explains it?
2. Give two fixes, one with a **raw identifier**, and say when you'd use each.
3. Why is there a *second* error, E0425, mentioning `r#gen`? What general rule about compiler error cascades does it
   illustrate?

### 14. Design exercise

**Meridian's workspace and build policy.** Meridian will run five Rust services within a year: the gateway, the fraud
feature library (called from Java via FFM), the market-data fan-out, and two internal tools. Design:

- **Repository structure:** one workspace or several? Where do shared crates (config, telemetry, error types) live?
- **Toolchain and MSRV policy**, and how upgrades roll out across teams.
- **Targets and CPU baselines** per service, including the FFM library, which is loaded into a JVM on whatever hardware
  Java runs on.
- **Artifact strategy:** musl static images vs glibc base images, and the allocator choice.

Justify each decision with its trade-off. State what would make you split or merge workspaces later.
