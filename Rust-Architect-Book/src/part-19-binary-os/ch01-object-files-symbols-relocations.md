# Chapter 19.1 — Object Files, Symbols, and Relocations

> **Where this sits:** Part XIX · Binary, Linker, and OS · chapter 1 of 6
> **Prerequisites:** Chapter 2.1 (crates, `.rlib`, targets), Chapter 7.1 (monomorphized instances, v0 symbols),
> Chapter 18.6 (codegen units, function merging), Chapter 18.7 (the `call qword ptr [rip + foo@GOTPCREL]` you saw
> leave the compiler).
> **After this chapter you can:** read an object file's sections, symbols, and relocations; explain what the linker
> does to each kind of reference (same crate, statically linked crate, shared library); say why rustc's calls into
> `std` stay indirect and what flag changes that; read the symbol table of a Rust binary, raw and demangled; decide what
> to strip from a release binary without losing the ability to symbolize a crash; and explain why a rebuild of the same
> commit can fail to match its own debug file.

---

## Pass 1 · User level — *What leaves the compiler, and what is still missing*

### 1. Problem

Part XVIII ended at an object file. Every function rustc compiled was machine code, but much of that machine code
couldn't run yet. A call to a helper in the same crate doesn't know where the helper will be placed. A call into
`std` doesn't know where `std`'s code will be. A call to `getpid` doesn't even know which *file* will provide it:
that is decided on the machine where the program eventually runs, possibly years after the build.

An object file is therefore machine code **with holes**, plus a list of instructions for filling them. The component
that fills them is the **linker** (at build time) and, for anything in a shared library, the **dynamic loader** (at
run time). Between them sit three ideas this chapter makes concrete:

- **Sections**: named byte ranges (`.text`, `.rodata`, `.data.rel.ro`, `.eh_frame`, ...) that the linker merges.
- **Symbols**: names for addresses, both the ones this file defines and the ones it needs from elsewhere.
- **Relocations**: "at offset X, write the address of symbol S (plus an adjustment), encoded like this."

Why should an architect care? Because these three things are how your release binary interacts with everything
*around* it: profilers read its symbols, crash reporting depends on keeping symbols or debug information, relocation
processing is part of startup cost and of the process's attack surface, and a missing or mismatched symbol is exactly
the kind of failure that surfaces at 02:00 on a machine you don't control.

### 2. Mental model

```text
 relax.rs ──rustc──► relax.o  (ELF "relocatable")                 relax  (ELF executable, PIE)
                     ┌──────────────────────────────────┐           ┌──────────────────────────────────┐
                     │ .text.<fn>   code with holes      │  linker   │ .text     all code, laid out      │
                     │ .rela.text.<fn>  fill-in list      │ ───────►  │ .got      address slots           │
                     │ .data.rel.ro, .rodata             │ (rust-lld)│ .rela.dyn what the LOADER still   │
                     │ .eh_frame    unwind rules         │           │           must fill in (Ch. 19.4) │
                     │ .symtab      defined + undefined  │           │ .dynsym   imports from .so files  │
                     └──────────────────────────────────┘           └──────────────────────────────────┘

 What happens to each kind of reference:

   reference to...                    relocation in .o        after static linking
   ─────────────────────────────────  ──────────────────────  ─────────────────────────────────────────────
   a function in the same crate       R_X86_64_PLT32          direct `call rel32` (hole filled, gone)
   a function in std / another crate  R_X86_64_GOTPCREL       `call *GOT(%rip)`; GOT slot gets a RELATIVE
     (statically linked)                                        relocation the loader applies at startup
   a function in libc.so (shared)     R_X86_64_GOTPCREL       `call *GOT(%rip)`; GOT slot gets GLOB_DAT:
                                                                the loader looks the symbol up by name
```

Symbols come in two flavors in every object: **defined** (this file provides the address) and **undefined** (this
file needs it). The linker's core job is to match every undefined symbol to exactly one definition, then compute
addresses and patch the holes. Anything it can't finish (because the definition lives in a shared library, or because
the executable will be loaded at a random address) becomes a **dynamic relocation** for the loader.

### 3. Rust code

Listing `ch01-01-object-to-executable.rs` compiles a small program with the `rustc` inside the Playground's container
and keeps both the object file and the executable. (The Playground sandbox includes `rustc`, `gcc`, and GNU binutils,
so every `readelf`, `objdump`, and `nm` output in this Part is real; see the Part overview.) The program touches all
three kinds of reference:

```rust,ignore
// The program compiled inside listing ch01-01 (its SRC constant).
#[inline(never)]
pub fn local_helper(x: u64) -> u64 { x.wrapping_mul(3) }      // defined in this crate
fn main() {
    let n = std::hint::black_box(14u64);
    let v = local_helper(n);                                    // call into this crate
    let p = unsafe { getpid() };                                // call into libc (a shared library)
    println!("{v} {}", p > 0);                                  // calls into std (another crate)
}
unsafe extern "C" { fn getpid() -> i32; }
```

The object file's symbol table (`nm`), trimmed to the interesting names. `T`/`t` are defined code (global/local), `U`
is undefined, meaning "someone else must provide this":

```text
0000000000000000 T _RINvNtCs9k3SxhrAWiO_3std2rt10lang_startuECs6dW3OWb2LLo_5relax
0000000000000000 t _RNvCs6dW3OWb2LLo_5relax12local_helper
0000000000000000 T _RNvCs6dW3OWb2LLo_5relax4main
                 U _RNvNtNtCs9k3SxhrAWiO_3std2io5stdio6__print
                 U getpid
0000000000000000 T main
```

Every defined address is `0`, because each function sits in its own section (`.text._RNvCs6dW3OWb2LLo_5relax4main`
and so on, one section per function [RUSTC]) and nothing has been placed yet. Two details from earlier Parts show up
here. `std::rt::lang_start::<()>` is **defined in your crate**, not in `std`, because it's a generic instance and
instances are compiled where they're used (Chapter 7.1). And the plain `main` symbol, a C-ABI function rustc generates
to call `lang_start`, is also yours (Chapter 19.4 follows it).

Here is `relax::main` in the object file, with its holes (`objdump -dr` prints each relocation under the instruction
it patches):

```text
  17:	e8 00 00 00 00       	call   1c <_RNvCs6dW3OWb2LLo_5relax4main+0x1c>
			18: R_X86_64_PLT32	.text._RNvCs6dW3OWb2LLo_5relax12local_helper-0x4
  21:	ff 15 00 00 00 00    	call   *0x0(%rip)        # 27 <_RNvCs6dW3OWb2LLo_5relax4main+0x27>
			23: R_X86_64_GOTPCREL	getpid-0x4
  38:	48 8b 05 00 00 00 00 	mov    0x0(%rip),%rax        # 3f <_RNvCs6dW3OWb2LLo_5relax4main+0x3f>
			3b: R_X86_64_GOTPCREL	_RNvXsd_NtNtNtCsgxBkk5gSRhY_4core3fmt3num3impyNtB9_7Display3fmt-0x4
  5a:	48 8d 3d 00 00 00 00 	lea    0x0(%rip),%rdi        # 61 <_RNvCs6dW3OWb2LLo_5relax4main+0x61>
			5d: R_X86_64_PC32	.Lanon.cbfbd0ccd29a91065fc2bb6c5782a21f.1-0x4
  66:	ff 15 00 00 00 00    	call   *0x0(%rip)        # 6c <_RNvCs6dW3OWb2LLo_5relax4main+0x6c>
			68: R_X86_64_GOTPCREL	_RNvNtNtCs9k3SxhrAWiO_3std2io5stdio6__print-0x4
```

(Trimmed: the stack bookkeeping between these instructions is omitted.) Every `00 00 00 00` is a hole. And the same
function after linking, where the holes are filled:

```text
   15057:	e8 d4 ff ff ff       	call   15030 <_RNvCs6dW3OWb2LLo_5relax12local_helper>
   15061:	ff 15 79 0b 04 00    	call   *0x40b79(%rip)        # 55be0 <getpid@GLIBC_2.2.5>
   15078:	48 8b 05 69 0b 04 00 	mov    0x40b69(%rip),%rax        # 55be8 <_DYNAMIC+0x240>
   1509a:	48 8d 3d af fe fe ff 	lea    -0x10151(%rip),%rdi        # 4f50 <__abi_tag+0x4c54>
   150a6:	ff 15 4c 0b 04 00    	call   *0x40b4c(%rip)        # 55bf8 <_DYNAMIC+0x250>
```

The call to `local_helper` became a plain relative `call`: the linker knew both addresses and wrote the distance
(`d4 ff ff ff` = −44). The calls to `getpid` and `std::io::_print` still go through memory: `call *0x40b79(%rip)` reads
an 8-byte **slot** at `0x55be0` and jumps to whatever address is stored there. Those slots are the **Global Offset
Table** (GOT), and the linker left instructions for filling them:

```text
0000000000055bf8  0000000000000008 R_X86_64_RELATIVE                         3b630
0000000000055be0  0000000600000006 R_X86_64_GLOB_DAT      0000000000000000 getpid@GLIBC_2.2.5 + 0
```

Slot `0x55bf8` (`_print`) gets `R_X86_64_RELATIVE 3b630`: "the load address of this executable plus `0x3b630`",
because `_print` *is* in this file (`std` was linked statically) but the file will be loaded at a random base
(Chapter 19.4). Slot `0x55be0` (`getpid`) gets `R_X86_64_GLOB_DAT getpid@GLIBC_2.2.5`: "look up the symbol `getpid`,
version `GLIBC_2.2.5`, in the loaded shared libraries, and write its address here." Across the whole executable:

```text
     67 R_X86_64_GLOB_DAT
      2 R_X86_64_JUMP_SLOT
    608 R_X86_64_RELATIVE
```

677 fixups the loader performs at every start of this small program. That's the bill for position independence and
shared libraries, and Chapter 19.4 shows the loader paying it.

## Pass 2 · Systems level — *Relocation types, the GOT, and the symbol table*

### 4. Under the hood

**Relocation types are encodings, not just targets.** A relocation says *how* to compute and write the value
[ELF x86-64 psABI]:

| Type | Computes | Used for |
|---|---|---|
| `R_X86_64_PC32` | S + A − P (32-bit, PC-relative) | `lea` of data in the same output (the format-string pieces) |
| `R_X86_64_PLT32` | L + A − P, where L is the PLT entry *or* the function itself if it's local | direct `call`s |
| `R_X86_64_GOTPCREL` | G + GOT + A − P: the distance to a GOT slot holding S | `call *slot(%rip)`, `mov slot(%rip)` |
| `R_X86_64_GOTPCRELX` / `REX_GOTPCRELX` | same as GOTPCREL, but **relaxable** | lets the linker rewrite the instruction |
| `R_X86_64_RELATIVE` (dynamic) | B + A: load base plus addend | GOT slots and pointers to this file's own code/data |
| `R_X86_64_GLOB_DAT` (dynamic) | the address of symbol S, found by name | GOT slots for symbols in shared libraries |

(S = symbol address, A = addend, P = address being patched, B = load base, G/GOT = slot offset/GOT address.)

**Relaxation.** Chapter 17.8 showed the compiler's side of this instruction, `call qword ptr [rip +
playground::opaque@GOTPCREL]` in Intel syntax, and noted that the linker *may* turn it into a direct call. Here is
when it does. The GOT indirection is necessary for `getpid`: its address is only known at run time. For `_print` it
isn't: `std` ended up in the same executable, so a direct call would work. Linkers can **relax** such an instruction
(rewrite `call *slot(%rip)` into a direct `call`), but only when the relocation says it's allowed: the `X` variants.
Listing `ch01-02-got-and-relaxation.rs` shows `gcc -fno-plt` doing it for C:

```text
--- gcc -O1 -fno-plt -c a.c: relocations in caller ---
   5:	ff 15 00 00 00 00    	call   *0x0(%rip)        # b <caller+0xb>
			7: R_X86_64_GOTPCRELX	helper-0x4
   d:	ff 15 00 00 00 00    	call   *0x0(%rip)        # 13 <caller+0x13>
			f: R_X86_64_GOTPCRELX	getpid-0x4
--- after linking a.o + b.o into a PIE ---
    112e:	67 e8 2b 00 00 00    	addr32 call 115f <helper>
    1136:	ff 15 a4 2e 00 00    	call   *0x2ea4(%rip)        # 3fe0 <getpid@GLIBC_2.2.5>
```

The 6-byte indirect call to `helper` became a 5-byte direct call plus a 1-byte `addr32` prefix as padding, so nothing
else had to move. The call to `getpid` stays indirect, because `getpid` really is in a shared library.

rustc, by contrast, emitted **plain** `R_X86_64_GOTPCREL` in listing ch01-01, and the linker left the calls into `std`
indirect. Listing `ch01-06-relaxation-flags.rs` builds one program three ways to show that this is a code-generation
choice [RUSTC] [VERSION] (the two `-Z` flags are unstable; the listing enables them on the Playground's stable
compiler with `RUSTC_BOOTSTRAP=1`, which is for inspection, not for production builds):

```text
=== default ===
relocations in r.o:  3 R_X86_64_64; 4 R_X86_64_GOTPCREL; 11 R_X86_64_PC32; 3 R_X86_64_PLT32;
   14ffc:	call   *0x40aee(%rip)        # 55af0 <_DYNAMIC+0x238>
=== -Z relax-elf-relocations=yes ===
relocations in r.o:  3 R_X86_64_64; 3 R_X86_64_GOTPCRELX; 11 R_X86_64_PC32; 3 R_X86_64_PLT32; 1 R_X86_64_REX_GOTPCRELX;
   14fcc:	addr32 call 3b550 <std[6c98fd8553dbae28]::io::stdio::_print>
=== -Z plt=yes ===
relocations in r.o:  3 R_X86_64_64; 1 R_X86_64_GOTPCREL; 11 R_X86_64_PC32; 6 R_X86_64_PLT32;
   14fcc:	call   3b550 <std[6c98fd8553dbae28]::io::stdio::_print>
```

The same listing prints the target's defaults from `rustc --print target-spec-json`: `"plt-by-default": false` and
`"relro-level": "full"`. rustc avoids the Procedure Linkage Table (the classic `call foo@plt` stub) and asks for
GOT-indirect calls, which fits "full RELRO": all GOT slots are filled at startup and then made read-only (§5, and
Chapter 19.2 verifies `BIND_NOW` in the binary). The price is that calls into statically linked crates stay indirect
unless relaxation is enabled. Whether that matters is a performance question with a mechanism answer (§6), and an
exercise.

> **What actually happens at the call site?** Listing `ch01-02` reads it from the running process. It finds the
> `ff 15 <disp32>` instruction inside a function that calls `libc::getpid`, computes the slot address, and reads the
> slot:
>
> ```text
> call_getpid at 0x61e733384ee0: `ff 15` at +1, disp32 = 0x52031
> GOT slot        0x61e7333d6f18  in mapping: r--p /playground/target/debug/playground
> slot contains   0x791fb0238b90  in mapping: r-xp /usr/lib/x86_64-linux-gnu/libc.so.6
> dlsym(getpid) = 0x791fb0238b90  same as slot: true
> ```
>
> The slot lives in a **read-only** mapping of the executable (`r--p`): the loader wrote it, then protected it. The
> value is an address inside libc's code, exactly what `dlsym` returns for `getpid`.

**The symbol table of a Rust binary.** Listing `ch01-03-symbols-and-strip.rs` counts the symbol kinds in its own
(debug) executable:

```text
    593 t     452 r     283 T      95 U      24 b      16 d      11 w       5 B       3 D       1 V       1 R
```

Lowercase letters are **local** symbols (visible only inside this file), uppercase are global; `t`/`T` code, `r`/`R`
read-only data, `d`/`D` data, `b`/`B` zero-initialized data (`.bss`), `U` undefined (resolved at run time from shared
libraries), `w` weak (optional: the program checks at run time whether it exists). 95 undefined symbols: this "static"
Rust binary imports 95 functions and variables from glibc and libgcc_s. The names are **v0-mangled** (Chapter 7.1):

```text
0000000000071f60 d DW.ref.rust_eh_personality
0000000000025f30 T _RNvCs1njKG4L9aB3_7___rustc12___rust_alloc
0000000000025f70 T _RNvCs1njKG4L9aB3_7___rustc35___rust_no_alloc_shim_is_unstable_v2
000000000001ee90 t _RNvCsbMrhfRjmHQB_10playground15validate_amount
0000000000025f10 T main
000000000005bc10 T rust_eh_personality
```

and `nm -C` (binutils 2.42 understands v0) demangles them:

```text
0000000000025f30 T __rustc::__rust_alloc
000000000001ee90 t playground::validate_amount
```

Three families of names deserve a sentence each:

- **`rust_eh_personality`** and its GOT-like reference `DW.ref.rust_eh_personality`: the **personality routine** the
  unwinder calls for every Rust frame during a panic. Chapter 19.3 shows where the unwind tables name it.
- **`__rust_alloc` and friends**: the **allocator shims**. Every `Box`, `Vec`, and `String` allocation in every crate
  calls `__rust_alloc`, but *which allocator* is only decided when the final binary is linked. rustc generates these
  shims once, in the final artifact, in a pseudo-crate named `__rustc` [RUSTC]. Listing `ch01-04-allocator-shims.rs`
  disassembles them:

  ```text
  --- __rust_alloc, default allocator ---
  0000000000024260 <__rustc[100742bb89c490cb]::__rust_alloc>:
     24260:	jmp    3b200 <__rustc[100742bb89c490cb]::__rdl_alloc>

  --- __rust_alloc, with #[global_allocator] ---
  0000000000015d20 <__rustc[100742bb89c490cb]::__rust_alloc>:
     15d28:	call   15f90 <<core[c0acaeba6ab4c2e0]::alloc::layout::Layout>::from_size_alignment_unchecked>
     15d37:	call   16370 <<custom_alloc[55f7c9276680af04]::Forwarding as core[c0acaeba6ab4c2e0]::alloc::global::GlobalAlloc>::alloc>
  ```

  With the default allocator, the shim is one `jmp` to `__rdl_alloc` (std's `System` allocator, which calls
  `malloc`). With a `#[global_allocator]`, it calls your `GlobalAlloc::alloc` (the second binary is a debug build,
  so the `Layout` construction isn't inlined). This is how a crate compiled years ago allocates through the mimalloc or
  jemalloc your service chose today (Chapter 15.5's topic, and the reason the counting allocator used since Part III
  works at all).
- **`__rust_no_alloc_shim_is_unstable_v2`**: a marker symbol whose name tells you what it is for. Code that tries to
  link Rust without letting rustc generate the shims (for example, by providing `__rust_alloc` by hand) hits it. It's
  unstable by design [RUSTC].

### 5. Memory

Sections split into two groups by one question: **does it get mapped when the program runs?**

| Section(s) | Mapped at run time? | Notes |
|---|---|---|
| `.text` | yes, `r-x` | code, shared between all processes running this binary (page cache) |
| `.rodata`, `.eh_frame`, `.eh_frame_hdr` | yes, `r--` | constants, unwind tables (Ch. 19.3) |
| `.data.rel.ro`, `.got`, `.dynamic` | yes, `rw-` then `r--` | written by the loader, then made read-only: **RELRO** |
| `.data`, `.bss` | yes, `rw-` | mutable statics; `.bss` occupies no file space |
| `.symtab`, `.strtab` | **no** | only tools read them |
| `.debug_*` | **no** | only debuggers, `addr2line`, and backtraces read them |

The last two rows are why stripping is safe for the running process and why it's a *deployment* decision, not a
performance one. Listing `ch01-03` makes three copies of its own binary:

```text
full 4927280 bytes: with debug_info not stripped
nodebug 613848 bytes: not stripped
bare 459696 bytes: stripped
```

The 4.9 MB "full" file is 90% debug information (most of it `std`'s, which ships with line tables [LIB]). `strip
--strip-debug` keeps the symbol table; `strip --strip-all` removes it too. None of this changes RSS: those bytes were
never loaded.

What *does* cost memory is relocation processing. Each `RELATIVE` or `GLOB_DAT` relocation writes 8 bytes into a page
that came from the file. Writing turns a shared, file-backed page into a **private copy** (copy-on-write, Chapter 19.5)
for this process. 677 relocations mostly land in `.data.rel.ro` and `.got`, a few pages; that's why those sections are
grouped together and then protected (RELRO). For a large program with hundreds of thousands of relocations (vtables
and function-pointer tables are relocations too, Chapter 6.4), the private pages add up per process: order of
megabytes, not gigabytes [OS].

### 6. CPU / OS

An indirect call through the GOT costs a load from a (hot, cached) slot and an indirect branch that the CPU's branch
predictor learns after the first execution [CPU]. A direct call costs neither. For a call that runs once per request,
the difference is noise; inside a hot loop that calls a small, non-inlined function from another crate millions of
times per second, it can show up. The mechanism predicts a small effect, and the only honest answer is to measure it on
your code (exercise, and Part XX).

Two more CPU/OS facts sit behind the outputs above:

- **Position independence costs almost nothing on x86-64.** RIP-relative addressing (`lea 0x..(%rip)`) makes
  position-independent code nearly free on this architecture, which is why PIE is the default everywhere (`"position-
  independent-executables": true` in the target spec). On 32-bit x86 it cost a register; that folklore outlived the
  architecture.
- **The loader must finish before `main`.** `BIND_NOW` (full RELRO) means every `GLOB_DAT` is resolved at startup
  rather than at first call. That moves a little work to startup and removes a writable table of code pointers from the
  running process. Chapter 19.4 counts the system calls involved.

## Pass 3 · Architect level — *Symbols as an operational interface*

### 7. Trade-offs

| Decision | Option | Gains | Costs |
|---|---|---|---|
| What to strip from shipped binaries | nothing | backtraces with file:line on the host; easy debugging | image size (4.9 MB vs 460 KB here); source paths and internal names ship to customers |
| | debug info only (`strip=debuginfo`) | function names in backtraces; `perf` works | no file:line without the separate debug file |
| | everything (`strip=symbols`) | smallest binary | backtraces are raw addresses; you *must* keep a matching debug file |
| Where debug info lives | inside the binary | always matches | bloats images, slows image pulls |
| | separate file keyed by build-id | small images, full symbolization offline | a pipeline: upload, retention, lookup |
| Relocation model | GOT-indirect, no PLT (rustc default) | full RELRO; no lazy-binding writable table | calls into other crates indirect unless relaxation is on |
| | relaxable relocations | direct calls where possible | an unstable flag today [VERSION]; measure before caring |
| Symbol visibility | everything global | easy debugging with any tool | larger dynamic symbol table in `cdylib`s (Chapter 19.2 shows rustc exports only `#[no_mangle]` items) |

> **Why not just ship `debug = true` binaries?** Size is one reason; information exposure is the other. A binary with
> full debug info carries your source file paths, type names, and often enough structure to reconstruct a lot of your
> code. For software shipped to third parties (an on-prem agent, a CLI) that's a disclosure decision. For your own
> servers it's mostly an image-size and pull-time decision. Either way, "strip and keep the debug file" gives you both.

### 8. Java comparison

A `.class` file is also code with holes. Its **constant pool** holds *symbolic* references (`java/lang/String.length:
()I`), and the JVM **resolves** each one lazily, the first time the instruction that uses it executes [JVMS §5.4.3].
That is dynamic linking per call site, with the class loader as the "dynamic loader", and it's why a missing class
surfaces as a `NoClassDefFoundError` minutes into a run instead of at startup. `invokedynamic` goes further and lets
the program decide the target at first call.

| | ELF + Rust | Class files + JVM |
|---|---|---|
| Unresolved reference | relocation against a symbol | constant-pool entry |
| When resolved | link time; the rest at load time (`BIND_NOW`) | lazily, at first execution of each site |
| Failure of a missing target | link error, or loader error at startup | `NoClassDefFoundError` / `NoSuchMethodError` at first use |
| Names at run time | only if you keep `.symtab`/debug info | always: class metadata *is* the program |
| Stack traces | need symbols or debug info | always have names and line numbers (`LineNumberTable`) |
| "Stripping" | `strip` / `-C strip` | obfuscation (ProGuard/R8) + a mapping file to de-obfuscate |

The last row is the closest analogy to build-ids: an Android team that ships R8-obfuscated code keeps
`mapping.txt` per build and uploads it to their crash reporter, exactly as you keep a `.debug` file per build-id.

> **Analogy limit.** The JVM's JIT-compiled machine code has no ELF symbols at all. That's why profilers need
> JVMTI agents (async-profiler) or `perf-map-agent` to learn which address range belongs to which method. A Rust
> binary is the opposite case: all code exists in the file before it runs, so standard tools (`perf`, `gdb`,
> `addr2line`) work directly, provided you kept the symbols or can find the debug file.

### 9. Production scenario

**Meridian's symbol pipeline.** After the market-data ingest rewrite (the Rust service from Chapter 8.2) went to
production, the platform team set a policy for all Rust services' release images:

1. Release profile: `debug = "line-tables-only"` (Chapter 18.6 §7) so the build *has* file:line information.
2. The CI job runs `objcopy --only-keep-debug` to extract a `.debug` file, then ships the binary stripped of debug
   information with a `.gnu_debuglink` pointer (Chapter 19.3 measures this: 4.5 MB → 460 KB).
3. The `.debug` file is uploaded to an internal **symbol server**, keyed by the binary's GNU **build-id** (a hash
   the linker writes into a note section; `readelf -n` prints it). Listing `ch01-03` shows the key survives stripping:
   the `full` and `bare` copies print the same `Build ID: a4e4f500821899d43a4936f7f6f5d55d3d10b50f`.
4. The crash pipeline symbolizes raw addresses offline. With the stripped copy, `RUST_BACKTRACE=full` prints only
   addresses (`0x5abaeed288d1 - <unknown>`), and `addr2line` against the matching debug file recovers
   `playground::validate_amount` at `/playground/src/main.rs:8`.

The policy's rule of thumb for engineers: **the running binary doesn't need names, but someone at 02:00 does**, so
names must be recoverable for every build that ever reached production, for as long as that build can crash.

### 10. Failure scenario

**The debug file that didn't match.** A few months later, a panic in market-data ingest's frame decoder produced a
stripped backtrace. The symbol server had no debug file for its build-id: that build's upload step had failed
silently. The on-call engineer did the obvious thing: checked out the same commit, rebuilt with the same toolchain and
flags, and tried to symbolize with the fresh debug file. The symbol server and the debugger both refused it: the
**build-ids differed**.

Listing `ch01-05-build-id-repro.rs` reproduces the cause. It builds one source file in two directories, as two CI
runners with different workspace paths would:

```text
no debug info        runner-a 4f2096ae2b10  runner-b 4f2096ae2b10  -> SAME
debug info           runner-a 347088f79965  runner-b a5142abc96cc  -> DIFFERENT
debug info + remap   runner-a 7256aa6edbd2  runner-b 7256aa6edbd2  -> SAME
```

Without debug info the two builds are bit-identical. With debug info they differ, because DWARF records the
compilation directory (the listing counts one occurrence of `/tmp/runner-a/svc` in the binary, and zero after
remapping). The engineer's rebuild ran in a differently named workspace, so it wasn't the same binary. Tools that
match debug files by build-id were right to refuse it. Forcing it with `addr2line` would have *happened* to work in
this case, because only a recorded directory differed, but nothing guaranteed that: any other drift (a different
dependency resolution, a toolchain patch release) gives plausible-looking wrong answers with no warning.
`--remap-path-prefix=$PWD=/build` makes the output independent of the directory.

What Meridian changed:

- `RUSTFLAGS` in the release pipeline includes `--remap-path-prefix` for the workspace and for `$CARGO_HOME` (registry
  sources are embedded the same way).
- The upload step is part of the release gate: no successful upload, no deploy.
- A weekly job rebuilds the last release from source on a different runner and compares build-ids. A mismatch is a
  reproducibility bug and gets a ticket.
- The on-call runbook says: *symbolize with the artifact from the build that crashed, never with a rebuild*.

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XIX).*

1. What three things does an object file contain besides machine code, and which of them survive into the running
   process?
2. Walk through what happens to a call to a function in the same crate, a function in `std`, and `getpid` from libc,
   from the object file to the running process.
3. What is the GOT, who writes it, and why is it read-only by the time `main` runs in a Rust binary?
4. What is relocation relaxation? Why did the linker turn gcc's call to `helper` into a direct call but leave rustc's
   calls into `std` indirect?
5. What are `__rust_alloc` and `__rdl_alloc`, where are they generated, and how does `#[global_allocator]` change
   them?
6. A colleague says "stripping symbols makes our service faster." What's true and false about that?
7. What is a GNU build-id, and why can a rebuild of the same commit have a different one?
8. Compare symbol resolution in ELF with constant-pool resolution in the JVM. When does each fail if a target is
   missing?

### 12. Exercises

- **Beginner.** Run `nm -C` on a release build of Project L1 (`logstat`). How many `T` symbols belong to `logstat`
  itself, to `std`, and to `core`? Which of `logstat`'s own functions are missing, and why (Chapter 18.6 on inlining)?
- **Intermediate.** Extend listing `ch01-01` to also call a function from a second crate you compile with
  `rustc --crate-type=rlib` and link with `--extern`. Which relocation type does the call get, and what does the linker
  leave in `.rela.dyn` for it?
- **Advanced.** Using `ch01-06`'s approach, build a program whose hot loop calls a non-inlined function in `std`
  (for example, `str::parse::<u64>` on short strings) with default relocations and with relaxation. Measure both
  (best of N, release) and decide whether the indirection matters for this workload.
- **Systems.** Count the dynamic relocations in the release build of a real service (`readelf -r` on Linux). Which
  sections do the `RELATIVE` ones land in? Estimate the number of pages each process privately copies at startup, and
  check your estimate against `Private_Dirty` in `/proc/<pid>/smaps` for those mappings.
- **Architecture.** Design the symbol and debug-information policy for three artifacts: an internal service, an
  on-prem agent shipped to customers, and the fraud library loaded into the JVM via FFM (Part XVI). Specify what ships,
  what is retained, where, for how long, and who can access it.

### 13. Debugging exercise

A service links a vendored C library that defines `hash_bytes`, and your crate also exports a
`#[unsafe(no_mangle)] pub extern "C" fn hash_bytes(...)` for a different purpose (a plugin interface). The build
succeeds. In production, some of the C library's lookups return wrong results, but only in the release build.

1. What does the linker do when two object files define the same global symbol, and why might it *not* report an
   error in this situation (think about archives: a `.a` member is pulled in only to resolve an undefined symbol)?
2. How would you confirm which definition each call site uses, with the tools from this chapter?
3. Propose two fixes, one on the Rust side and one on the C side, and say which you'd choose.

### 14. Design exercise

**A reproducible-build guarantee for Meridian's Rust services.** Specify what "reproducible" means for Meridian
(bit-identical binaries? identical build-ids? identical `.text`?), which inputs must be pinned (toolchain, lockfile,
environment variables, paths, timestamps, the linker), and how to verify it continuously (§10's weekly job is a
start). Include the trade-off with build caching (Chapter 7.3) and state which failure modes the guarantee protects
against: mismatched debug files, supply-chain tampering detection, and "works on my machine" incidents.
