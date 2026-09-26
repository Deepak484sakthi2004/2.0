# Chapter 19.2 — Static and Dynamic Linking

> **Where this sits:** Part XIX · Binary, Linker, and OS · chapter 2 of 6
> **Prerequisites:** Chapter 19.1 (symbols, relocations, the GOT), Chapter 2.1 (targets, musl vs glibc, the gateway's
> build policy), Chapter 6.5 (the C-ABI plugin vtable), Chapter 18.2 (proc macros).
> **After this chapter you can:** say exactly which parts of a Rust binary are linked statically and which dynamically;
> predict the oldest Linux a binary will start on, and why weak references don't help; read the dynamic loader's
> search for a library; explain symbol interposition and when it can't happen; choose among dynamic glibc, static glibc,
> musl, and a shared `libstd` for each kind of artifact; and explain what a `cdylib` exports and how a proc macro gets
> into the compiler.

---

## Pass 1 · User level — *Copy the code in, or find it at run time?*

### 1. Problem

Chapter 19.1's debug binary had 95 undefined symbols: `getpid`, `pthread_key_create`, `__libc_start_main`, and more.
Something has to supply them. There are two ways:

- **Static linking.** The linker copies the needed code from an archive into the executable. The binary is
  self-contained, and whatever it links is frozen at build time.
- **Dynamic linking.** The linker records a dependency (`NEEDED libc.so.6`) and a list of symbols to look up. The
  **dynamic loader** (`ld-linux-x86-64.so.2`) finds the library on the target machine at every start and binds the
  symbols. The binary is small, and the library can be patched without rebuilding it, but the program now depends on
  what's installed where it runs.

Java engineers mostly never make this decision, because the JVM makes it for them. Rust engineers make it per artifact,
often without noticing, and it determines things that matter in production: **which machines the binary starts on**,
**who patches a libc vulnerability** (the OS team or you), **how big the image is**, **how fast the process starts**,
and **whether tools like `LD_PRELOAD`-based profilers can hook into it**.

### 2. Mental model

What goes into a default Rust binary on `x86_64-unknown-linux-gnu`:

```text
 ┌──────────────────────── playground (ELF, PIE) ─────────────────────────┐
 │  your crates ─┐                                                         │
 │  dependencies ├── from .rlib files: ALWAYS statically linked            │    at run time, found by
 │  std, core ───┘   (Rust has no stable ABI, so Rust code is copied in)   │    the dynamic loader:
 │                                                                         │ ──► libc.so.6     (glibc)
 │  NEEDED: libgcc_s.so.1, libc.so.6, ld-linux-x86-64.so.2                 │ ──► libgcc_s.so.1 (unwinder)
 │  version needs: GLIBC_2.2.5 ... GLIBC_2.39 (the build machine's glibc)  │ ──► ld-linux (the loader itself)
 └─────────────────────────────────────────────────────────────────────────┘
```

The spectrum of choices, from most dynamic to fully static:

| Build | Rust code | C runtime | Result | Typical use |
|---|---|---|---|---|
| `-C prefer-dynamic` | `libstd-<hash>.so` shared | glibc shared | 5 KB executable + a toolchain-specific `.so` | compiler internals, experiments; never ship |
| default `*-linux-gnu` | static | glibc shared | PIE, needs a glibc at least as new as the build's | most services on distro base images |
| `-C target-feature=+crt-static` (gnu) | static | glibc static | static-pie; glibc's NSS/`dlopen` caveats | rare; see §7 |
| `*-linux-musl` | static | musl static | fully static; no loader at all | `FROM scratch` images, CLIs (Meridian's gateway, Ch. 2.1) |
| `--crate-type=cdylib` | static inside the `.so` | glibc shared | a C-ABI shared library | plugins, the fraud library loaded by the JVM |

### 3. Rust code

Listing `ch02-02-static-vs-dynamic.rs` builds the same small program twice with the Playground's `rustc`, once by
default and once with `-C target-feature=+crt-static`, and compares them (`-C strip=symbols` on both):

```text
--- hello-dyn ---
368336 bytes
ELF 64-bit LSB pie executable, x86-64, dynamically linked
	linux-vdso.so.1 (0x000072767fdf5000)
	libgcc_s.so.1 => /lib/x86_64-linux-gnu/libgcc_s.so.1 (0x000072767fd5f000)
	libc.so.6 => /lib/x86_64-linux-gnu/libc.so.6 (0x000072767fb4c000)
--- hello-static ---
1464432 bytes
ELF 64-bit LSB pie executable, x86-64, static-pie linked
	statically linked
```

The static binary is four times larger, because it carries its own copy of the parts of glibc it uses. It's still a
PIE ("static-pie"): it relocates *itself* at startup, so it still gets address-space randomization (Chapter 19.4).

Then it starts each binary 200 times (one run on a shared machine: noisy, order-of-magnitude only):

```text
hello-dyn    cargo's environment   : 978 µs per spawn+run+wait (200 runs)
hello-static cargo's environment   : 643 µs per spawn+run+wait (200 runs)
hello-dyn    without LD_LIBRARY_PATH: 877 µs per spawn+run+wait (200 runs)
hello-static without LD_LIBRARY_PATH: 511 µs per spawn+run+wait (200 runs)
```

About a third of a millisecond per start is the dynamic loader's work, and a long `LD_LIBRARY_PATH` (cargo sets one
when it runs a program) adds another tenth. A second run of the same listing gave 887, 388, 716, and 406 µs: the
static binary started faster every time, while the exact numbers moved by 20–40%, which is what "one run, noisy" means
on a shared machine. Chapter 19.4 traces those starts system call by system call.

Finally, **symbol interposition**. A tiny C library defines its own `getpid` that returns 4242, and each binary runs
with `LD_PRELOAD` pointing at it:

```text
LD_PRELOAD=libfake.so hello-dyn: pid via std::process::id() = 4242
LD_PRELOAD=libfake.so hello-static: pid via std::process::id() = 917
```

In the dynamic binary, `std::process::id()` calls `getpid` through a GOT slot (Chapter 19.1), the loader fills that slot
by searching the loaded objects in order, and a preloaded library comes first. The static binary has nothing to look
up, so the preload has no effect.

## Pass 2 · Systems level — *Archives, version needs, and the loader's search*

### 4. Under the hood

**Static linking is selective copying.** An `.rlib` (Chapter 2.1) and a C `.a` are archives of object files. The
linker pulls in an archive member only if it defines a symbol something still needs, and rustc emits **one section per
function** (Chapter 19.1) so the linker's `--gc-sections` can then drop every function nothing reaches. That's why a
"hello world" that links all of `std` is 368 KB stripped, not the size of `std`'s rlib. The linker itself is, since
Rust 1.90, **rust-lld** by default on this target [VERSION]. Listing `ch02-05-linkers-and-musl.rs` reads the linker's
signature out of the `.comment` section and compares it with GNU ld:

```text
=== default: best of 3 rustc runs 223 ms, 4548344 bytes ===
.comment: GCC: (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0;Linker: LLD 22.1.8 (/checkout/src/llvm-project/llvm 52ed14fcd56afc30f9cccd8ca8ce237c2eef7e04);
dynamic FLAGS:  (FLAGS) BIND_NOW; (FLAGS_1) Flags: NOW PIE;
RELRO segment: 1
=== GNU ld (-C linker-features=-lld): best of 3 rustc runs 304 ms, 4498512 bytes ===
.comment: GCC: (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0;
dynamic FLAGS:  (FLAGS) BIND_NOW; (FLAGS_1) Flags: NOW PIE;
RELRO segment: 1
```

(Whole `rustc` invocations, best of 3, one run: noisy. The difference is link time, and it grows with binary size:
this is why rustc switched.) Both linkers produce `BIND_NOW` and a `GNU_RELRO` segment: **full RELRO** is the target
default (`"relro-level": "full"`, Chapter 19.1). The `GCC:` string comes from the C runtime start files (`crt1.o` and
friends), which are still compiled by gcc and linked in either way.

**Dynamic linking is a contract with versions.** glibc doesn't just export `pthread_key_create`; it exports
`pthread_key_create@GLIBC_2.34`. Every symbol your binary imports is bound to a **version**, and the binary records,
per library, the list of versions it needs (`.gnu.version_r`). At startup the loader checks that list *before running
anything*. Listing `ch02-01-glibc-floor.rs` reads its own requirements:

```text
glibc versions required (15 distinct):
  GLIBC_2.39   pidfd_spawnp (weak), pidfd_getpid (weak)
  GLIBC_2.34   __libc_start_main, pthread_key_create, pthread_key_delete, pthread_attr_getstack, pthread_setspecific, pthread_attr_getguardsize
  GLIBC_2.33   fstat64, stat64
  GLIBC_2.32   pthread_getattr_np
version needs for libc.so.6 (readelf -V), newest three:
Name: GLIBC_2.33  Flags: none  Version: 6
Name: GLIBC_2.34  Flags: none  Version: 2
Name: GLIBC_2.39  Flags: none  Version: 9
```

Two facts here determine where this binary can run:

1. **`__libc_start_main@GLIBC_2.34`.** glibc 2.34 (2021) merged libpthread into libc and gave the process start
   routine a new version, so every program linked against glibc ≥ 2.34 needs at least 2.34 [LIB, attributed: glibc 2.34
   release notes]. Nothing in your code causes it; the start files do.
2. **The weak references don't help.** `std` references `pidfd_spawnp` and `pidfd_getpid` *weakly* (it checks at run
   time whether they exist; they're used by `Command` on new kernels). But the version need `GLIBC_2.39` is recorded
   with `Flags: none`, not `WEAK`, so the loader treats it as mandatory. Listing `ch02-05` links a small
   `Command`-using program with both linkers, and both do the same:

   ```text
   default: Name: GLIBC_2.39  Flags: none  Version: 9
   GNU ld (-C linker-features=-lld): Name: GLIBC_2.39  Flags: none  Version: 3
   ```

The listing can't install an old glibc, so it simulates one: it renames a required version inside a copy of itself
(same length, so the file layout doesn't change) and lets the real loader try:

```text
--- copy with GLIBC_2.39 renamed to GLIBC_2.99 (2 occurrence(s) of the string) ---
exit status 1
/tmp/needs-GLIBC_2.99: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.99' not found (required by /tmp/needs-GLIBC_2.99)
```

That message is exactly what a machine with glibc 2.38 or older prints for a real binary built on this Playground
image. The rule to take away: **the glibc on the build machine sets the floor, and the floor is the newest mandatory
version need**, not the newest function you meant to call.

**How the loader finds a library.** Listing `ch02-03-shared-libraries.rs` builds a C library `libfee.so` in a private
directory and links a Rust program against it. Without telling the loader where to look:

```text
exit status 127
./app/use_fee: error while loading shared libraries: libfee.so: cannot open shared object file: No such file or directory
```

Exit status 127 and that message are the loader's, not your program's: `main` never ran. With a `RUNPATH` of
`$ORIGIN/lib` (resolved relative to the executable's own location) it works from any current directory, and
`LD_DEBUG=libs` shows the search:

```text
	find library=libfee.so [0]; searching
	 search path=/tmp/app/lib/glibc-hwcaps/x86-64-v4:/tmp/app/lib/glibc-hwcaps/x86-64-v3:/tmp/app/lib/glibc-hwcaps/x86-64-v2:/tmp/app/lib		(RUNPATH from file /tmp/app/use_fee)
	  trying file=/tmp/app/lib/glibc-hwcaps/x86-64-v4/libfee.so
	  trying file=/tmp/app/lib/glibc-hwcaps/x86-64-v3/libfee.so
	  trying file=/tmp/app/lib/glibc-hwcaps/x86-64-v2/libfee.so
	  trying file=/tmp/app/lib/libfee.so
```

Note the `glibc-hwcaps/x86-64-v4`, `-v3`, `-v2` subdirectories: glibc (since 2.33) looks first for builds of the library
optimized for the CPU's feature level, so a distribution can ship AVX2 and baseline versions side by side [LIB]. It's
the loader-level cousin of the runtime feature detection Chapter 2.1 recommended after the SIGILL incident. The full
order is: `LD_LIBRARY_PATH`, then the executable's `RUNPATH`, then `/etc/ld.so.cache`, then the default directories
(and `DT_RPATH`, the older mechanism, before `LD_LIBRARY_PATH` when no `RUNPATH` exists) [LIB: `man ld.so`].

**What a Rust shared library exports.** The same listing builds a `cdylib` with one `#[unsafe(no_mangle)] pub extern
"C"` function and one ordinary `pub fn`:

```text
4463600 bytes
defined dynamic symbols: 1
00000000000129a0 T risk_score
internal_weight in .symtab: 1
```

Only `risk_score` is in the dynamic symbol table, the table other programs can bind to. The `pub fn` exists in the
file (the static symbol table still lists it) but isn't exported [RUSTC]. `pub` is a Rust-level visibility; a
`cdylib`'s ABI surface is exactly its `#[no_mangle]` / `#[export_name]` items. Loading it at run time uses `dlopen` and
`dlsym`, the same calls the JVM's `System.loadLibrary` and FFM's `SymbolLookup.libraryLookup` use (Part XVI):

```text
risk_score(250_000) = 250
librisk.so mappings in this process: 4
```

**Rust's own uses of dynamic linking.** Listing `ch02-04-rust-dylibs.rs` shows two:

```text
--- 1. -C prefer-dynamic: std becomes a NEEDED shared library ---
hi-static-std 349976 bytes
hi-dyn-std 5344 bytes
 0x0000000000000001 (NEEDED)             Shared library: [libstd-64f5f36fb0927694.so]
exit status 127
./hi-dyn-std: error while loading shared libraries: libstd-64f5f36fb0927694.so: cannot open shared object file: No such file or directory
```

The hash in `libstd-64f5f36fb0927694.so` is the point: Rust has no stable ABI, so a shared `std` is only valid for the
exact compiler build that produced it. No distribution ships it as a system library, and that's why Rust links Rust
code statically. And the second use:

```text
--- 2. rustc loads a proc-macro crate with dlopen ---
ELF 64-bit LSB shared object, x86-64
0000000000055c80 D __rustc_proc_macro_decls_9730d7b23fd9b960__
	file=/tmp/libanswer_macro.so [0];  dynamically loaded by /playground/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib/librustc_driver-eaf3ed1d23ca5027.so [0]
	calling init: /tmp/libanswer_macro.so
answer!() = 42
```

A proc-macro crate (Chapter 18.2) is compiled into a **shared library for the host**, exporting one data symbol, the
table of macros it defines. The compiler, itself mostly `librustc_driver-<hash>.so`, `dlopen`s it and calls into it
while expanding your code. That's why proc macros run with the compiler's full privileges on your build machine, and
why they must be built for the host even when you cross-compile.

### 5. Memory

Dynamic linking's classic argument is memory sharing: one copy of libc's code in RAM, mapped into every process. That's
real. The executable-code mapping of `libc.so.6` in listing `ch05-01-address-space.rs` is 1,572 KiB, backed by the page
cache and shared with every other process on the machine that uses the same file. A statically linked binary carries
its own copy of the libc parts it uses, but those pages are *also* shared among all processes running **that same
binary**. So the sharing argument is strongest for many *different* programs using the same libraries (a desktop, a
build host) and weakest for a container running one service binary, which is the usual Rust deployment.

The costs that don't depend on sharing: each process's **private dirty pages** from relocation processing (Chapter
19.1 §5), which a static-pie binary has too (it relocates itself), and the loader's own mappings (`ld-linux` is about
230 KiB of mappings in the same listing).

### 6. CPU / OS

At startup the dynamic loader opens each `NEEDED` library, `mmap`s its segments, applies relocations, resolves every
`GLOB_DAT` (because of `BIND_NOW`), runs initializers, and applies RELRO protections. Chapter 19.4's trace shows those
steps as system calls: `openat`, `read`, `fstat`, several `mmap`s, and `mprotect`s per library. With cargo's long
`LD_LIBRARY_PATH`, the same trace shows 40 failed `openat` calls as the loader tries each directory in turn: the
+100 µs above.

After startup, a call into libc costs an indirect call through the GOT (Chapter 19.1 §6); a static binary calls
directly. Neither matters for code that makes a system call anyway, since the system call costs hundreds of
nanoseconds (Chapter 19.6 measures one).

## Pass 3 · Architect level — *A linking policy per artifact*

### 7. Trade-offs

| Criterion | Dynamic glibc (default) | Static glibc (`+crt-static`) | musl static | 
|---|---|---|---|
| Where it starts | glibc ≥ the build machine's newest mandatory version need | any Linux with a compatible kernel | any Linux with a compatible kernel |
| libc security fixes | the OS team patches `libc.so.6`; no rebuild | rebuild and redeploy | rebuild and redeploy |
| Image | needs a base image with glibc (distroless, Debian, ...) | `FROM scratch` possible | `FROM scratch` (Meridian's gateway) |
| Name resolution (NSS), `dlopen` | full glibc behavior (`nsswitch.conf`, LDAP, mDNS modules) | NSS and `dlopen` need the *same* glibc's shared modules at run time [LIB] | musl's own simpler resolver; `dlopen` limited |
| `malloc` | glibc's | glibc's | musl's, notably slower under multithreaded load: swap in mimalloc/jemalloc (Ch. 2.1) |
| Interposition (`LD_PRELOAD` profilers, heap checkers) | works | doesn't | doesn't |
| Startup | loader work (§3: ~0.35 ms here) | self-relocation only | self-relocation only |
| Verifiable on the Playground | yes | yes | no: the musl `std` isn't installed there (E0463 in listing `ch02-05`) |

> **Why not static-link glibc everywhere, then?** Because glibc isn't designed for it. Name resolution through NSS
> (`getaddrinfo` with `nsswitch.conf` modules), `iconv`, and anything that uses `dlopen` load *shared* glibc
> components at run time, and they must match the glibc version the binary was built with [LIB]. In listing `ch02-02`
> the static binary resolved `localhost` fine (`Ok(2)`), because it ran on the machine it was built on. That's
> precisely the situation that doesn't hold in production. If you want one file that runs anywhere, musl is the
> designed-for-it option; static glibc is the "works until the resolver configuration changes" option.

> **Why doesn't Rust share `libstd` between programs?** No stable ABI (Chapter 6.5): the layout of `String`, the
> calling convention of Rust functions, and every generic instance can change between compiler versions, so a shared
> `libstd` must come from the exact same compiler build. The `-C prefer-dynamic` binary above was 5 KB and useless on
> any machine without that exact `libstd-64f5f36fb0927694.so`.

### 8. Java comparison

The JVM *is* a dynamic linker, one that works at class granularity. The class path (or module path) is its
`LD_LIBRARY_PATH`; class loaders are its search order; "jar hell" (two versions of a library on the class path, first
one wins) is the same failure family as symbol interposition. `System.loadLibrary("fraud")` searches
`java.library.path` and calls `dlopen` underneath, which is how the fraud library gets into the JVM process. On the
"static" end, `jlink` builds a trimmed runtime image with only the modules an application uses, and GraalVM Native
Image can produce a fully static executable (`--static --libc=musl`), the same trade-off this chapter describes.

| | Rust on Linux | Java |
|---|---|---|
| Unit of linking | symbol | class |
| Search path | `LD_LIBRARY_PATH`, `RUNPATH`, `ld.so.cache` | class path / module path, class loader delegation |
| Two versions of a library in one process | not by default: one global symbol namespace (`dlopen` with `RTLD_LOCAL` isolates) | possible with separate class loaders (OSGi, app servers) |
| "Runs on older platform?" | glibc floor from the build machine | class file version (`--release 17`) |
| Fully static option | musl target | GraalVM Native Image `--static` |

> **Analogy limit.** `javac --release 17` makes the *compiler* refuse APIs newer than 17, so the class files run on
> every Java 17 runtime by construction. There's no equivalent flag for glibc: rustc and the linker don't know which
> glibc your fleet has, and the floor is whatever the build machine's glibc makes it. You get the Java behavior only by
> **building on the oldest glibc you support** (or with a toolchain that targets an older glibc explicitly; not
> verified here).

### 9. Production scenario

**Meridian's linking policy.** After the incident in §10, platform engineering wrote down one policy per artifact
type:

- **Gateway and other edge services:** `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`, fully static,
  mimalloc as the global allocator, `FROM scratch` images (Chapter 2.1's policy, now with the reasons written down: no
  glibc floor, a ~10 MB image, no NSS configuration to drift, and nothing for a compromised container to `dlopen`).
- **payments-core and services with system C dependencies:** default `*-linux-gnu`, built in a builder image whose
  glibc equals the **oldest** glibc in the fleet, on distroless base images. The release gate audits the glibc floor
  against the fleet inventory (the Part XIX review capstone builds that audit).
- **The fraud feature library (FFM):** a `cdylib` built in the same oldest-glibc builder, because it's loaded into the
  JVM's process and must accept that process's glibc. CI checks that `nm -D --defined-only` lists exactly the exported
  functions in the library's header, so an accidental `#[no_mangle]` can't grow its ABI surface.
- **Developer tools and CLIs:** musl static binaries, so one download runs on every engineer's Linux machine.
- **Never shipped:** `-C prefer-dynamic`, and `LD_LIBRARY_PATH` in production manifests (it's a debugging tool, and it
  cost ~0.1 ms per process start in §3 anyway).

### 10. Failure scenario

**The settlement job that didn't start.** Meridian's settlement batch job (the Rust job from Chapter 8.1) runs nightly
on a small pool of VMs in the payments network segment. Those VMs were still on an older LTS release with glibc 2.31,
because that segment's change process is slow. The CI system moved its runners to a new image with glibc 2.39, and the
next release of the job was built there. Tests passed in CI. At 02:00, the job's systemd unit failed immediately:

```text
settlement: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.34' not found (required by settlement)
settlement: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.39' not found (required by settlement)
```

(Illustrative; the format is the loader's, verified in listing `ch02-01`.) The job's code hadn't changed in any way
that touched glibc. The new floor came from the start files (`__libc_start_main@GLIBC_2.34`) and from `std`'s weak
`pidfd` references (mandatory `GLIBC_2.39`), exactly the two facts in §4. The failure was total but at least loud:
exit status 1, nothing ran, and the alert fired. Settlement completed four hours late after an engineer rebuilt the
job on an old image by hand.

The lessons Meridian recorded:

1. **The build environment is part of the artifact.** The job's builder image is now pinned to the oldest glibc in its
   deployment targets, and changing it is a reviewed change with a fleet check.
2. **Audit the floor in CI.** A release gate reads the mandatory version needs (`readelf -V`, as in the review
   capstone) and compares the newest one with an inventory of target hosts.
3. **Batch jobs that run on hosts you don't control go musl-static.** The settlement job moved to the musl target the
   next quarter, with mimalloc, and the glibc floor stopped being its problem.
4. **Staging must match production's OS, not just its application config.** The staging VMs had been upgraded; the
   production segment hadn't.

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XIX).*

1. Which parts of a default Rust binary on `x86_64-unknown-linux-gnu` are statically linked, which dynamically, and
   why does Rust statically link Rust code?
2. What determines the oldest glibc a Rust binary will start on? Why don't weak references lower it?
3. What does `error while loading shared libraries: libfee.so: cannot open shared object file` tell you about *when*
   the failure happened, and which exit status does the process get?
4. Explain symbol interposition with `LD_PRELOAD`. Why did it change `std::process::id()` in the dynamic binary and
   not in the static one?
5. What does a `cdylib` export, and how is that different from what `pub` means in Rust?
6. Why is static glibc discouraged, while musl static binaries are common?
7. How does a proc macro get into the compiler, and what does that imply for build security and cross-compilation?
8. Compare the glibc floor problem with Java's `--release` flag. Where does the analogy break?

### 12. Exercises

- **Beginner.** Run `ldd` and `readelf -d` on three binaries on your machine: a Rust release build, a Go binary, and
  `/usr/bin/java`. Which are dynamically linked, against what, and what's each one's glibc floor?
- **Intermediate.** Extend listing `ch02-03` so the program links against `libfee.so` with a `RUNPATH`, then ship a
  second, incompatible `libfee.so` (same name, `fee_bps` renamed) in `LD_LIBRARY_PATH`. Predict and observe which
  library wins and what the error looks like.
- **Advanced.** Build the `Command`-using program from §4 on an old glibc (a container with glibc 2.31) and on a new
  one. Compare `readelf -V` output. Does the old build still use `pidfd_spawnp` on a new system? (Read `std`'s source for
  how it detects the function at run time.)
- **Systems.** On a machine with many different processes, compare `Pss` for `libc.so.6`'s mappings (from
  `/proc/<pid>/smaps_rollup` and `smaps`) across processes. Quantify how much memory dynamic linking saves for that
  machine, and then for a Kubernetes node running 30 copies of one static service.
- **Architecture.** Write the linking policy for a company with three runtime environments: Kubernetes clusters you
  control, customer-managed VMs (an on-prem agent), and AWS Lambda. For each, choose the target, allocator, and base
  image, and state the patching story for a libc CVE.

### 13. Debugging exercise

A Rust service works in staging and dies in production at startup: exit status 1, and a one-line error from the
dynamic loader on stderr. The container image is the same digest in both environments. Production's manifest adds `LD_LIBRARY_PATH=/opt/vendor/lib` for a sidecar's benefit
(the sidecar shares the pod's environment template), and `/opt/vendor/lib` in production contains an old
`libgcc_s.so.1`.

1. Why does a variable meant for another process affect this one, and why only in production?
2. What would `LD_DEBUG=libs` show, and what would the error message probably be?
3. Give two fixes: one in the manifest and one in how the Rust binary is built. Which one prevents the whole class of
   failure?

### 14. Design exercise

**Choosing the artifact type for Meridian's new "risk rules" engine**, which must be callable from the Java ledger
(via FFM), from a Rust service (as a library), and as a standalone CLI for analysts. Decide the crate types (`rlib`,
`cdylib`, `staticlib`, binary), how each is linked, what the C ABI surface is and how it's versioned (symbol versions?
a version function? `SONAME`?), how the `cdylib` handles panics at the boundary (Chapter 8.3), and how you'll test that
the same logic produces the same results through all three entry points.
