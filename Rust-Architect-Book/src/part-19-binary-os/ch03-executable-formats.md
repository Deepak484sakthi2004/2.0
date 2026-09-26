# Chapter 19.3 — Executable Formats: ELF, PE, Mach-O

> **Where this sits:** Part XIX · Binary, Linker, and OS · chapter 3 of 6
> **Prerequisites:** Chapter 19.1 (sections, symbols, relocations), Chapter 19.2 (the dynamic loader, `NEEDED`),
> Chapter 8.3 (panics, unwinding, landing pads, `extern "C"` aborting on unwind since 1.81), Chapter 18.6 (debug-info
> settings in profiles).
> **After this chapter you can:** read an ELF file's two views (segments for the kernel, sections for tools) with a
> parser you wrote; say which bytes of an executable are ever loaded into memory; find the unwind tables, the
> personality routine, and the LSDA behind a Rust landing pad, and explain why an `extern "C"` call site has none;
> choose a debug-information and frame-pointer policy with measured sizes; and compare ELF with PE/COFF, Mach-O, and a
> WebAssembly module.

---

## Pass 1 · User level — *What the kernel reads, and what only tools read*

### 1. Problem

Chapters 19.1 and 19.2 treated the executable as a bag of sections. The kernel doesn't see it that way. When you run
a program, the kernel reads a few hundred bytes of **headers**, maps some byte ranges of the file into memory with
specific permissions, and jumps to an address. Everything else in the file, sometimes 90% of it, is never loaded: it's
there for linkers, debuggers, profilers, and crash tooling.

That split is the key to three decisions an architect owns:

- **What to ship.** Debug information, symbol tables, and unwind tables have different costs and serve different
  consumers. Getting this wrong means either bloated images or crashes you can't diagnose.
- **How stacks are walked.** Panics (Chapter 8.3), backtraces, `catch_unwind` at FFI boundaries, and sampling
  profilers all need to find the caller of the current function. There are two mechanisms, unwind tables and frame
  pointers, with different costs.
- **Which platform formats you'll meet.** Linux uses ELF, Windows PE/COFF, macOS Mach-O, and WebAssembly has its own
  module format. The ideas carry over; the tools and failure modes differ.

### 2. Mental model

```text
 ELF file                                          two views of the same bytes
 ┌───────────────────────┐
 │ ELF header (64 bytes) │── e_entry, e_phoff, e_shoff, e_type (DYN = PIE), e_machine (62 = x86-64)
 ├───────────────────────┤
 │ program headers       │── SEGMENTS: the loader's view. "map file range X at address Y with permissions P"
 │   PHDR INTERP LOAD×4  │      LOAD R      headers, .rodata, .eh_frame     (read-only)
 │   DYNAMIC TLS RELRO   │      LOAD R E    .text                          (executable)
 │   EH_FRAME STACK NOTE │      LOAD RW     .data.rel.ro, .got, .dynamic   (RELRO: read-only after relocation)
 ├───────────────────────┤      LOAD RW     .data, .bss                    (memsz > filesz: .bss is zeroed)
 │ .text .rodata .data   │
 │ .eh_frame .got ...    │
 │ .symtab .strtab       │── never mapped: tools only
 │ .debug_info .debug_*  │── never mapped: debuggers, addr2line, backtraces
 ├───────────────────────┤
 │ section headers       │── SECTIONS: the linker's and tools' view (names, types, sizes)
 └───────────────────────┘
```

Sections are how the **linker** and **tools** think; segments are how the **kernel** and **loader** think. A section
belongs to at most one segment; many sections belong to none.

### 3. Rust code

Listing `ch03-01-elf-parser.rs` is a small ELF64 reader (about 150 lines, with two unit tests) that parses the
running program's own file. The core of reading program headers is plain offset arithmetic over a byte slice:

```rust
    fn segments(&self, h: &Header) -> Vec<Segment> {
        (0..h.phnum as usize)
            .map(|i| {
                let o = h.phoff as usize + i * h.phentsize as usize;
                let b = self.bytes;
                Segment {
                    kind: u32_at(b, o),
                    flags: u32_at(b, o + 4),
                    offset: u64_at(b, o + 8),
                    vaddr: u64_at(b, o + 16),
                    filesz: u64_at(b, o + 32),
                    memsz: u64_at(b, o + 40),
                }
            })
            .collect()
    }
```

Its output for the debug build of itself:

```text
file 5131552 bytes, type 3 (3 = ET_DYN: PIE), machine 62 (62 = x86-64)
entry 0x1bc80, 12 program headers at 0x40, 43 section headers at 0x4e4260
segment          offset     vaddr    filesz     memsz flags
PHDR               0x40      0x40     0x2a0     0x2a0 R  
INTERP            0x2e0     0x2e0      0x1c      0x1c R  
LOAD                0x0       0x0   0x1ac80   0x1ac80 R  
LOAD            0x1ac80   0x1bc80   0x501e0   0x501e0 R E
LOAD            0x6ae60   0x6ce60    0x35b8    0x41a0 RW 
LOAD            0x6e418   0x71418     0x9c8     0xa92 RW 
TLS             0x6ae60   0x6ce60      0x30      0x50 R  
DYNAMIC         0x6d8d8   0x6f8d8     0x1d0     0x1d0 RW 
GNU_RELRO       0x6ae60   0x6ce60    0x35b8    0x41a0 R  
GNU_EH_FRAME    0x11314   0x11314    0x1d74    0x1d74 R  
GNU_STACK           0x0       0x0       0x0       0x0 RW 
NOTE              0x2fc     0x2fc      0x44      0x44 R  
interpreter: /lib64/ld-linux-x86-64.so.2
needed: ["libgcc_s.so.1", "libc.so.6", "ld-linux-x86-64.so.2"]
largest sections: .debug_str 2107933, .debug_info 1273448, .debug_ranges 552464, .debug_line 504697, .text 328035, .strtab 145159
bytes in sections that get mapped (addr != 0): 456640; the rest is never loaded
load base 0x594e71ca9000; AT_ENTRY - load base = 0x1bc80; matches e_entry: true
```

and for the release build (`// verify: release ok`, same listing):

```text
file 488552 bytes, type 3 (3 = ET_DYN: PIE), machine 62 (62 = x86-64)
largest sections: .text 271475, .strtab 84977, .symtab 27576, .rodata 24576, .eh_frame 22856, .rela.dyn 18144
bytes in sections that get mapped (addr != 0): 374713; the rest is never loaded
```

The debug file is 5.1 MB, of which 457 KB is ever mapped. The four largest sections are all debug information, and
none of them reaches memory. The last line cross-checks the parse against the kernel: the auxiliary vector (Chapter
19.4) says where the kernel mapped the program headers, which gives the load base, and the entry point the kernel will
jump to minus that base equals `e_entry` from the file.

## Pass 2 · Systems level — *Segments, unwind tables, and debug information*

### 4. Under the hood

**Reading the segment table.**

- **Four `LOAD` segments with different permissions** give the classic layout: read-only headers and data, an
  executable text segment, and two writable ones. The first writable `LOAD` coincides exactly with `GNU_RELRO` (same
  offset and size): it's written by the loader during relocation and then `mprotect`ed read-only (Chapter 19.4 shows
  the `mprotect` calls). No segment is both writable and executable (**W^X**).
- **`memsz > filesz`** (`0x41a0` vs `0x35b8`, and `0xa92` vs `0x9c8`): the difference is zero-initialized memory
  (`.bss`, `.tbss`) that takes no space in the file.
- **`vaddr` differs from `offset`** by `0x1000` or `0x2000` in later segments: the linker aligns segments so each can be
  mapped with page granularity while keeping the file compact.
- **`TLS`** is the template for thread-local storage (`thread_local!`, Chapter 11.4): each new thread gets a copy.
- **`GNU_EH_FRAME`** points at `.eh_frame_hdr`, a sorted table that lets the unwinder binary-search for the unwind
  rules of any code address.
- **`GNU_STACK`** carries no data; its flags say whether the stack must be executable. `RW` here, as it should be; the
  review capstone finds a binary where it isn't.
- **`NOTE`** holds the GNU build-id (Chapter 19.1) and the ABI tag.
- **`INTERP`** names the dynamic loader. A fully static binary (musl) has no `INTERP` and the kernel jumps straight to
  its entry point.

**Unwind tables, concretely.** Chapter 8.3 said a panic unwinds by consulting tables and running **landing pads**.
Listing `ch03-02-unwind-tables.rs` compiles one function three ways and shows those tables in the object file:

```rust,ignore
// The function compiled inside listing ch03-02 ("ABI" is replaced by "C" or "C-unwind" per build).
pub struct Guard(pub u32);
impl Drop for Guard {
    fn drop(&mut self) { unsafe { release(self.0) } }
}
unsafe extern "C" { fn release(id: u32); }
unsafe extern "ABI" { fn may_fail(x: u32) -> u32; }

#[unsafe(no_mangle)]
pub fn with_guard(x: u32) -> u32 {
    let _g = Guard(x);                 // dropped on the normal path, and during unwinding if may_fail unwinds
    (unsafe { may_fail(x) }) + 1
}
```

Build A declares `may_fail` as `extern "C"` and uses `panic=unwind`:

```text
=== A: callee extern "C", panic=unwind ===
sections: .eh_frame 
<with_guard>:
	push   %rbp
	push   %rbx
	push   %rax
	mov    %edi,%ebx
	call   *0x0(%rip)        # <with_guard+0xb>
			R_X86_64_GOTPCREL	may_fail-0x4
	mov    %eax,%ebp
	inc    %ebp
	mov    %ebx,%edi
	call   *0x0(%rip)        # <with_guard+0x17>
			R_X86_64_GOTPCREL	release-0x4
	mov    %ebp,%eax
	add    $0x8,%rsp
	pop    %rbx
	pop    %rbp
	ret
```

There's no landing pad and no `.gcc_except_table`. Since Rust 1.81 an `extern "C"` function **can't unwind** (an
unwind reaching that boundary aborts, Chapter 8.3) [LANG] [VERSION], so the compiler knows the guard can only be dropped
on the normal path. Build B declares the callee `extern "C-unwind"`, which may unwind:

```text
=== B: callee extern "C-unwind", panic=unwind ===
sections: .eh_frame .gcc_except_table.with_guard 
<with_guard>:
	push   %r14
	push   %rbx
	push   %rax
	mov    %edi,%ebx
	call   *0x0(%rip)        # <with_guard+0xc>
			R_X86_64_GOTPCREL	may_fail-0x4
	inc    %eax
	mov    %ebx,%edi
	mov    %eax,%ebx
	call   *0x0(%rip)        # <with_guard+0x18>
			R_X86_64_GOTPCREL	release-0x4
	mov    %ebx,%eax
	add    $0x8,%rsp
	pop    %rbx
	pop    %r14
	ret
	mov    %rax,%r14
	mov    %ebx,%edi
	call   *0x0(%rip)        # <with_guard+0x2d>
			R_X86_64_GOTPCREL	release-0x4
	mov    %r14,%rdi
	call   <with_guard+0x35>
			R_X86_64_PLT32	_Unwind_Resume-0x4
```

Everything after the `ret` is the **landing pad**: it's never reached by normal control flow. If `may_fail` unwinds,
the unwinder transfers control there with the exception object in `rax`; the pad saves it, runs the guard's `Drop`
(`release`), and calls `_Unwind_Resume` to continue unwinding into the caller. The tables that make this possible:

```text
0000002c 000000000000001c 00000000 CIE
  Augmentation:          "zPLR"
  Augmentation data:     9b c1 ff ff ff 1b 1b
0000004c 0000000000000030 00000024 FDE cie=0000002c pc=0000000000000000..0000000000000035
  Augmentation data:     a3 ff ff ff
=== B: relocations in .eh_frame (what each CIE/FDE points at) ===
R_X86_64_PC32 .text._RNvXCs1VpfnutnQzw_6guardBNtB2_5GuardNtNtNtCsgxBkk5gSRhY_4core3ops4drop4Drop4drop
R_X86_64_PC32 DW.ref.rust_eh_personality
R_X86_64_PC32 .text.with_guard
R_X86_64_PC32 .gcc_except_table.with_guard
```

Reading it:

- A **CIE** (Common Information Entry) holds rules shared by many functions. This one's augmentation string `"zPLR"`
  says it carries a **P**ersonality routine, that FDEs have an **L**SDA pointer, and the pointer encoding (**R**). Its
  personality relocation points at `DW.ref.rust_eh_personality`: Rust's personality routine, the function the unwinder
  calls for every Rust frame to ask "do you have a landing pad for this address?" (Chapter 19.1 found it in the symbol
  table).
- An **FDE** (Frame Description Entry) covers one function's address range (`pc=0..0x35`, all of `with_guard`) and
  describes, instruction by instruction, how to find the caller's frame (where the return address is, which registers
  were saved where). Its augmentation data is a pointer to the **LSDA**.
- The **LSDA** (Language-Specific Data Area, here `.gcc_except_table.with_guard`) is a few bytes of call-site records:
  "if an exception passes through the call at offsets A..B, land at pad C." It's 12 bytes:

  ```text
    0x00000000 ffff0108 06062200 0c290000          ......"..)..
  ```

The first CIE in the same file (`"zR"`, no personality) covers the `Drop::drop` function, which calls nothing that
can unwind.

Build C keeps the `C-unwind` callee but compiles with `panic=abort`:

```text
=== C: callee extern "C-unwind", panic=abort ===
sections: .eh_frame .gcc_except_table.with_guard 
	ret
	lea    0xc(%rsp),%rdi
	call   <with_guard+0x2f>
			R_X86_64_PLT32	.text._RINvNtCsgxBkk5gSRhY_4core3ptr9drop_glueNtCscLsKfXHm15R_6guardC5GuardEBD_-0x4
	call   *0x0(%rip)        # <with_guard+0x35>
			R_X86_64_GOTPCREL	core[c0acaeba6ab4c2e0]::panicking::panic_cannot_unwind-0x4
```

(Trimmed to the pad.) Even with `panic=abort`, a foreign exception can arrive through a `C-unwind` call, so the
compiler still emits a landing pad. This one runs the guard's drop glue and then calls `panic_cannot_unwind`, which
aborts. `panic=abort` means *Rust* panics abort; it doesn't make foreign unwinding impossible.

Unwind **tables** (`.eh_frame`) are emitted in every build here, abort or not: the target spec says `"default-uwtable":
true` (listing `ch01-06`) [RUSTC]. That's deliberate. Backtraces, debuggers, and DWARF-based profilers use the same
tables to walk the stack, so removing them would cost observability rather than just unwinding.

**Debug information as a deployment decision.** Listing `ch03-03-debuginfo.rs` builds one small program seven ways
(`-C opt-level=2`), runs each with `RUST_BACKTRACE=1` into a panic, and records what the backtrace frame for
`settle()` shows:

```text
variant    exe bytes .debug other files                  backtrace frame for settle()
d0           4508944      6 -                            2: settle::settle  3: settle::main
lines        4514576      7 -                            2: settle::settle  at /tmp/settle.rs:4:20
full         4533904      8 -                            2: settle::settle  at /tmp/settle.rs:4:20
packed       4512728      8 settle.dwp:21192             2: settle::settle  at /tmp/settle.rs:4:20
unpacked     4512736      8 settle.settle.bf2bbd40888b55d4-cgu.0.rcgu.dwo:20848 2: settle::settle  at /tmp/settle.rs:4:20
strip-dbg     461744      1 -                            2: settle::settle  3: settle::main
strip-sym     352600      1 -                            (no frame named settle)
```

Four things stand out:

1. **`debuginfo=0` still produces a 4.5 MB binary with six `.debug_*` sections.** Those are the precompiled `std`'s
   line tables [LIB]: your crate contributed nothing, the standard library did. `-C strip=debuginfo` removes them:
   462 KB.
2. **`line-tables-only` gets you `file:line`** in backtraces for a few KB more than `debuginfo=0` here, which is why
   Meridian's release profile uses it (Chapter 19.1 §9).
3. **`split-debuginfo`** moves *your crate's* debug information out of the executable: `packed` writes one `.dwp` file
   (via the `dwp` tool, which the Playground image has), `unpacked` leaves one `.dwo` per codegen unit. The executable
   still carries `std`'s sections here, and backtraces still resolve lines, because the `.dwp` sits next to the binary.
4. **`strip=symbols` removes names entirely**: the frame for `settle` disappears from the backtrace. You need an
   external debug file to learn anything.

The distro approach, separating everything into a file linked by name and build-id, is the last part of the listing:

```text
settle 4533904
settle.stripped 459760
settle.debug 4183792
  [     0]  settle.debug
build-ids:  2 d8f180a0ea0dc077366dd990dbb1fbabe72b1f98 
addr2line via the .debug file: settle::settle /tmp/settle.rs:3
```

`settle.stripped` carries a `.gnu_debuglink` section naming `settle.debug`, and both files have the same build-id (the
`2` is `uniq -c`'s count), which is how debuggers and symbol servers match them.

**Frame pointers vs unwind tables.** A profiler that samples a thread 100 times a second needs the call stack at each
sample, cheaply. Walking `.eh_frame` means interpreting DWARF rules per frame; the cheap alternative is the **frame
pointer** chain: each function saves the caller's `rbp` and sets `rbp` to its own frame, making the stack a linked list.
Release builds on x86-64 Linux omit it by default [RUSTC]. Listing `ch03-05-frame-pointers.rs` compares a function's
prologue:

```text
=== default  ===
<fp[bd50b43fc921520d]::frame_hash>:
	push   %r15
	push   %r14
	push   %r13
	push   %r12
	push   %rbx
.text bytes: 290
.eh_frame bytes: 104
=== forced -C force-frame-pointers=yes ===
<fp[bd50b43fc921520d]::frame_hash>:
	push   %rbp
	mov    %rsp,%rbp
	push   %r15
	push   %r14
	push   %r13
	push   %r12
	push   %rbx
.text bytes: 296
.eh_frame bytes: 104
```

Two instructions and six bytes for this function, and one fewer general-purpose register for the optimizer. The unwind
tables are the same size either way: frame pointers are an *addition* for profilers, not a replacement.

**Other formats.** The same concepts, different containers:

| | ELF (Linux, BSDs) | PE/COFF (Windows) | Mach-O (macOS, iOS) | WebAssembly module |
|---|---|---|---|---|
| Loader's view | program headers (segments) | section table + optional header | load commands (`LC_SEGMENT_64`) | sections; the host runtime instantiates it |
| Imports from libraries | `.dynsym` + GOT, resolved by `ld.so` | import table + IAT, resolved by the Windows loader | two-level namespace: each import names its dylib; `dyld` | explicit `import` section; the host supplies functions |
| Unwinding | `.eh_frame` (DWARF CFI) + LSDA | `.pdata`/`.xdata` tables (SEH) | compact unwind + DWARF | none native (exceptions proposal separate) |
| Debug info | DWARF in the file or `.dwo`/`.dwp`/`.debug` | PDB file, separate by design | DWARF in a separate `.dSYM` bundle | DWARF custom sections, or none |
| ASLR opt-in | PIE (`ET_DYN`) | `/DYNAMICBASE` | PIE by default | not applicable: linear memory |
| Verified in this chapter | yes | no: needs `dumpbin /headers` or `llvm-readobj` on a Windows target | no: needs `otool -l` on macOS | yes (below) |

The Playground image has the `wasm32-unknown-unknown` target installed, so listing `ch03-04-wasm-module.rs` builds a
`#![no_std]` library exporting one function and reads the module by hand (LEB128 integers and all):

```text
fee.wasm: 338 bytes, magic [00, 61, 73, 6d] ("asm"), version 1
  section  1 type          6 bytes  
  section  3 function      2 bytes  
  section  5 memory        3 bytes  
  section  6 global        9 bytes  
  section  7 export       20 bytes  memory (memory 0), fee_bps (func 0)
  section 10 code         14 bytes  
  section  0 custom       48 bytes  name "name"
  section  0 custom       61 bytes  name "producers"
  section  0 custom      148 bytes  name "target_features"
```

No program headers, no interpreter, no relocations to apply, no system calls: the module declares a type, a function,
a linear memory, and its exports, and the **host** (a browser, `wasmtime`, a blockchain VM in Chapter 25.4) provides
everything else. It's what an executable format looks like when the OS is replaced by an embedding API.

### 5. Memory

The rule from the parser's output: **only sections inside `LOAD` segments cost memory, and only the pages you touch
become resident** (Chapter 19.5). For the debug build, 5.1 MB on disk and 457 KB mappable; for the release build, 489 KB
on disk and 375 KB mappable. `.eh_frame` (23 KB in the release build) *is* mapped, but its pages are only faulted in
when something unwinds or walks the stack, and they're shared between processes because they're read-only and
file-backed.

`.bss` is the opposite case: it costs memory but no disk. A `static BUFFER: [u8; 1 << 20]` of zeros makes the file no
bigger and the `memsz` 1 MiB larger; the pages are allocated on first write.

### 6. CPU / OS

`execve` reads the ELF header and program headers, checks `e_machine`, maps each `LOAD` segment with its permissions,
maps the interpreter named by `INTERP`, sets up the stack (Chapter 19.4), and jumps to the interpreter's entry point,
or to `e_entry` for a static binary. It never looks at section headers: you can strip them entirely and the program
still runs. The permission bits become page-table bits, and the CPU enforces them: executing a page without the `E`
flag raises a fault (the NX bit on x86-64) [CPU] [OS].

Frame pointers cost a register. x86-64 has 16 general-purpose registers, and reserving `rbp` leaves 15 for the
register allocator (Chapter 17.8). The effect on performance is small in most code (fewer spills when a hot function
needs many registers), and it's workload-dependent: predicted from mechanism, measured in Part XX's exercises.

## Pass 3 · Architect level — *Shipping the right bytes*

### 7. Trade-offs

| Decision | Options (sizes from listing `ch03-03`, one small program) | Recommendation for services |
|---|---|---|
| Debug info in the build | `0` / `line-tables-only` / `2` (4.51 / 4.51 / 4.53 MB unstripped) | `line-tables-only` for release; `2` for debug builds |
| Where it lives | in the binary / `split-debuginfo=packed` (.dwp) / `objcopy --only-keep-debug` + debuglink | separate file, uploaded by build-id |
| What ships | unstripped / `strip=debuginfo` (462 KB) / `strip=symbols` (353 KB) | `strip=debuginfo` keeps names for `perf` and backtraces; full strip only when you have the pipeline |
| Stack walking for profilers | unwind tables only / plus frame pointers (+2 instructions per function) | frame pointers on for services under continuous profiling |
| Unwind tables | always emitted by default (`default-uwtable`) | keep them; they cost disk, not steady-state memory |
| Unwinding across FFI | `extern "C"` (aborts on unwind since 1.81) / `extern "C-unwind"` (landing pads) | `C` unless you explicitly design for unwinding (Chapter 8.3) |

> **Why not `panic=abort` to "remove the unwinding overhead"?** Look at build C: the landing pads you'd save are cold
> code after the `ret`, and the tables stay because `default-uwtable` keeps them for backtraces and profilers.
> `panic=abort` is a *semantic* choice (a panic ends the process, no `catch_unwind`, Chapter 8.3), with a small code-size
> benefit. Choose it for those semantics, not for speed.

### 8. Java comparison

A `.class` file is an executable format too: magic `0xCAFEBABE`, a version, the constant pool (symbols, Chapter 19.1),
and one entry per method whose `Code` attribute holds bytecode plus:

- an **`exception_table`**: "for bytecode offsets A..B, if the exception is of type T, jump to handler C". That's the
  Java counterpart of the LSDA's call-site records, and both implement "zero-cost" exceptions: nothing runs on the happy
  path, and a table lookup happens only when something throws.
- a **`LineNumberTable`**: bytecode offset → source line, the counterpart of DWARF line tables. Java keeps it by
  default (`javac -g:none` removes it), which is why every Java stack trace has line numbers.
- a **`StackMapTable`**: type states at branch targets for the verifier. Rust has no counterpart because its code is
  never verified at load time; the compiler's checks happened before the binary existed.

A JAR is a ZIP of such files; the JVM itself is an ELF program (`libjvm.so` plus a small launcher), so everything in
this chapter applies to the JVM process around your Java code.

> **Analogy limit.** Java stack traces come from the JVM's own frame metadata, for interpreted and JIT-compiled code
> alike, so they're always available and always symbolized. Rust stack walking depends on what you shipped: unwind
> tables for correctness, symbols or debug info for names, frame pointers for cheap profiling. A Java engineer's
> intuition that "a stack trace will tell me" holds for Rust only if the build policy made it true.

### 9. Production scenario

**Meridian's observability build policy.** The platform team's continuous profiler samples every production pod and
walks stacks by frame pointer, because DWARF unwinding of every sample was too expensive at fleet scale (not verified
here: the profiler is a third-party agent). For Rust services the policy became:

- `debug = "line-tables-only"`, `split-debuginfo` handled by `objcopy` in CI, debug files uploaded by build-id
  (Chapter 19.1 §9).
- Ship with `strip = "debuginfo"`: symbols stay, so the profiler and `perf` can name functions without the debug
  server, and panics print function names on the host. The binary goes from 4.5 MB to under 0.5 MB in listing
  `ch03-03`'s example.
- `-C force-frame-pointers=yes` for all services (+2 instructions per non-leaf function, listing `ch03-05`). The
  throughput cost was measured on the gateway before the rollout (Part XX's methods) and accepted.
- Keep `panic = "unwind"` for services (per-request panic containment, Chapter 8.3) and `extern "C"` at every FFI
  boundary unless a design review approves `C-unwind`.

### 10. Failure scenario

**The flame graph that blamed `memcpy`.** Before the frame-pointer policy, the fraud team profiled their scoring
service with the fleet profiler and saw a flame graph whose widest root-level frame was `memcpy`, with no callers. They
spent a sprint shaving copies from their feature-vector code (Chapter 9.5), and p99 didn't move.

The profile was wrong, not the service. The profiler walked frames by `rbp`, and the release build had no frame
pointers (listing `ch03-05`'s default prologue), so any sample taken inside a function that used `rbp` as a general
register produced a broken chain: the walk stopped early or jumped into garbage, and many samples were attributed to
whatever leaf function was on top. `memcpy` is a leaf that runs often, so it collected them.

The fix had three parts:

1. Rebuild with `-C force-frame-pointers=yes`; the flame graph then showed the real hot path, a
   `HashMap<String, f64>` lookup per feature (the one Chapter 9.5's production scenario later removed).
2. Know the limit of the fix: frames inside precompiled code (the prebuilt `std`, system libraries) have frame pointers
   only if *they* were built with them. Check your toolchain and distribution, or use DWARF unwinding for those
   investigations (not verified here: `perf record --call-graph dwarf`).
3. Add a canary to the profiling setup: a known synthetic hot path in a test service whose flame graph is checked after
   every profiler or toolchain change.

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XIX).*

1. What's the difference between an ELF segment and an ELF section? Which does the kernel use?
2. In the parser's output, why is `memsz` larger than `filesz` for the writable segments, and why does the first
   writable `LOAD` have the same range as `GNU_RELRO`?
3. What are the CIE, the FDE, the personality routine, and the LSDA? Which of them does a Rust panic consult, and in
   what order?
4. Why did build A (`extern "C"` callee) have no landing pad, while build C (`panic=abort`) had one?
5. A 4.5 MB release binary built with `debuginfo=0`: where do its debug sections come from, and what removes them?
6. Frame pointers vs unwind tables: what does each cost and who uses each?
7. How do PE/COFF and Mach-O handle debug information differently from ELF, and why does that matter for a crash
   pipeline?
8. Compare a Java class file's `exception_table` and `LineNumberTable` with ELF's LSDA and DWARF line tables.

### 12. Exercises

- **Beginner.** Extend listing `ch03-01` to print the name of every section that lies inside each `LOAD` segment
  (a section belongs to the segment whose `[offset, offset + filesz)` contains it). Check your result against
  `readelf -l`'s "Section to Segment mapping".
- **Intermediate.** Extend the parser to read `.note.gnu.build-id` and print the build-id, and to read `.gnu_debuglink`
  when present. Test it against listing `ch03-03`'s `settle.stripped`.
- **Advanced.** Add a nested call and a second guard to listing `ch03-02`'s build B. Predict how many call-site records
  the LSDA will contain and what each landing pad does, then check with `readelf -x .gcc_except_table...`.
- **Systems.** Measure the runtime cost of `-C force-frame-pointers=yes` on a CPU-bound benchmark of your choice
  (release, best of N, Part XX's method), and on a register-hungry function (many live values). Relate the results to
  §6's mechanism.
- **Architecture.** Your company ships a Rust CLI to Windows, macOS, and Linux customers and wants symbolized crash
  reports from all three. Design the build and symbol pipeline: formats, where debug information goes on each platform
  (PDB, dSYM, `.debug`), what identifies a build on each, and what the crash reporter uploads.

### 13. Debugging exercise

A service built with `panic = "abort"` links a C++ library through `extern "C-unwind"` declarations, because the C++
code can throw. In production, a C++ exception thrown during a rare error path kills the process with an abort. The
team expected the Rust code between the C++ frames to be unwound through and the exception caught by a C++ handler
further up the stack.

1. Using build C's landing pad, explain what happened when the C++ exception reached the Rust frame.
2. What would have happened with `panic = "unwind"` (build B)? With `extern "C"` declarations instead?
3. What design would you recommend for mixing Rust and C++ code where C++ can throw, and where should exceptions be
   caught?

### 14. Design exercise

**Meridian's stack-walking contract.** Several consumers walk stacks in Meridian's Rust services: panics with
`catch_unwind` at request boundaries, backtraces in logs, the continuous profiler, `gdb` on core dumps, and the fraud
library's FFM boundary. For each consumer, state what it needs from the binary (unwind tables, symbols, debug
information, frame pointers), design the build profile that satisfies all of them at the lowest cost, and write the CI
checks (in the style of the Part XIX review capstone) that prevent a future change from silently breaking one of them.
