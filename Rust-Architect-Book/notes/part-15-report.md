# Part 15 report — first half (15.1–15.3, README, answers 15.1–15.3)

## Status

- The first Part XV writer finished Chapter 15.1 and verified the 15.1 and 15.2 listings, then a safety classifier
  stopped it while it was writing 15.2's prose (the partial file was deleted). A resume writer added the `ch03-*`
  listings, then stopped on an API error before writing prose.
- This pass wrote **15.2, 15.3, the Part README, and the answer key for 15.1–15.3**, with the defensive framing
  (every UB example minimal, paired with its Miri/lint detection and the fix; no exploitation detail). The
  classifier didn't trigger again.
- The second half (15.4–15.6, `review.md`, answers for 15.4–15.6 + review + interview mode) is a separate writer's
  job: see `notes/part-15b-report.md` and `notes/part-15b-answers.md`.

**For the integrator:**
1. Append `notes/part-15b-answers.md` to `src/appendix/answers-part-15.md`, replacing its last line
   (`*Chapters 15.4–15.6, the Part XV review capstone, and interview mode continue below.*`).
2. Update the listing totals in `src/part-15-unsafe/README.md` ("Chapters 15.1–15.3 use 58 files and 98 checks…")
   once 15.4–15.6 land.
3. Add Part XV to `SUMMARY.md` only when both halves are complete.

## SUMMARY.md lines

Replace the Part XV draft block with (the 15.4–15.6 and review lines come from `notes/part-15b-report.md`):

```markdown
- [Part XV Overview](part-15-unsafe/README.md)
  - [15.1 What unsafe Means: Soundness and Invariants](part-15-unsafe/ch01-soundness-invariants.md)
  - [15.2 Raw Pointers, Provenance, and Aliasing Models](part-15-unsafe/ch02-raw-pointers-provenance.md)
  - [15.3 MaybeUninit, ManuallyDrop, and UnsafeCell](part-15-unsafe/ch03-maybeuninit-manuallydrop-unsafecell.md)
```

Appendix entry, after the preceding Part's answers line:

```markdown
  - [Part XV Answers](appendix/answers-part-15.md)
```

## PROGRESS concepts

| Concept | Where | Full treatment planned |
|---|---|---|
| Five superpowers; unsafe defines vs discharges; edition-2024 `unsafe_op_in_unsafe_fn`, `unsafe extern` + `safe` items, `#[unsafe(no_mangle)]` (1.82 syntax) | 15.1 | XVI |
| Soundness defined; validity vs safety invariants table; library UB vs language UB | 15.1 | — |
| Invalid bool returns 2 (release), `test`+`cmovne` variant returns 10; null ref, uninit int, invalid char, misaligned read (release OK / debug check / Miri) | 15.1 | — |
| Non-UTF-8 `str`: release "capacity overflow", debug precondition, Miri "entering unreachable code" in core validations | 15.1 | — |
| Debug precondition checks (1.78+, "optional, cannot be relied on"); `invalid_from_utf8_unchecked`, `useless_ptr_null_checks` lints | 15.1 | XV.6 |
| #25860 status on 1.98.1: classic snippet rejected, higher-ranked variant compiles (Miri: dangling) | 15.1 | XVIII.3 |
| Unsafe code may trust private fields and unsafe traits, never safe traits (`ExactSizeIterator` liar → heap overflow; `TrustedLen`) | 15.1 | — |
| Module = unit of trust (safe `restore` broke `HeaderBlock`); enforcement table (type / run-time / debug / unsafe fn / unsafe trait) | 15.1 | — |
| Pointer = address + provenance; same address, different provenance (native reads `b`, Miri UB; Miri spaces allocations) | 15.2 | XVI |
| `add` → `getelementptr inbounds nuw`, `wrapping_add` → plain GEP (debug IR); both `lea` in release, merged | 15.2 | XVIII.6 |
| OOB arithmetic UB without deref (Miri "in-bounds pointer arithmetic failed") | 15.2 | — |
| Strict provenance (1.84): `addr`/`with_addr`/`map_addr`; tagged pointer Miri-clean in both models; exposed provenance warning | 15.2 | — |
| Stacked Borrows mechanics (tags, retags, per-location stack) + Tree Borrows (Reserved/Active/Frozen/Disabled); POPL 2020 / PLDI 2025 | 15.2 | XV.6 |
| `as_mut_ptr()` twice: UB under SB, OK under TB; std `split_at_mut` shape (one pointer) OK in both | 15.2 | — |
| `container_of` via `&field`: UB under SB (retag covers `[0x8..0x10]` only), OK under TB; `&raw const (*whole).field` passes both | 15.2 | XV.6 (intrusive lists) |
| Two `&mut` to one element: SB error at creation, TB error at use ("foreign write" → Disabled); `get_disjoint_mut` shape (bounds + pairwise distinct) | 15.2 | — |
| `noalias` exploited: `transfer(&mut, &mut)` returns 70 while memory holds 100 (release asm, no reload); debug 100/100 | 15.2 | XX |
| `set_len` after init via `spare_capacity_mut`; `invalid_reference_casting` deny lint | 15.2 | — |
| Frame header: raw cast (release `0xfeca`/`704643072`, debug misaligned abort, Miri alignment UB) vs safe `from_be_bytes` (cmp + load + `bswap`) vs `zerocopy` (`U32<BigEndian>`, align 1) | 15.2 | XXIII.5 |
| Low-bit vs high-bit pointer tags; canonical addresses; TBI/LAM; CHERI and strict provenance | 15.2 | XIX |
| Three wrappers = three switched-off assumptions; niche table: `Option<MaybeUninit<&u8>>` 16, `Option<ManuallyDrop<&u8>>` 8, `Option<UnsafeCell<NonZeroU32>>` 8 vs 4 | 15.3 | — |
| `*p = v` on uninit slot drops garbage (Miri in `raw_vec` via `Vec<u8>` Drop); `write` vs assignment | 15.3 | — |
| `mem::uninitialized` today: 0x01 fill (`u64 = 0x0101…`, `bool = true`), panics for `&u64` | 15.3 | — |
| Panic during init: leak (0 drops) vs guard (2 drops); `write` then `init += 1` order | 15.3 | XV.4 |
| `ManuallyDrop` for drop order and Vec round trip; `Vec::into_raw_parts` stable on 1.98; `ptr::read` double drop (Miri use-after-free) | 15.3 | XVI.4 |
| `MyCell` over `UnsafeCell`; shared-write via helper evades lint, Miri "SharedReadOnly"; `!Sync` E0277 inherited | 15.3 | XI.4 |
| Why `UnsafeCell` hides niches (tag would change behind `&`) | 15.3 | — |
| Drop check: plain Drop E0597; std Vec `#[may_dangle]`; nightly `dropck_eyepatch`; PhantomData<T> keeps element drop checked; without it: compiles + UAF (Miri) | 15.3 | — |
| Zeroing cost: `resize` → `memset` in asm; measured 166–175 vs 287–293 ns per 16 KiB read (3 runs); pooled buffer kept initialized; `BorrowedBuf` unstable (E0658, #117693) | 15.3 | XX.4 |

## Promises to later Parts

- **Part XVI:** `unsafe extern` blocks with `safe` items (15.1) in real bindings; `Vec::into_raw_parts` / `from_raw_parts`
  and `Box::into_raw` across the C boundary (15.3); exposed provenance for addresses coming from C (15.2 §6).
- **Part XVIII (18.6):** `getelementptr inbounds nuw` from `ptr::add` and LLVM merging identical functions (15.2 §4).
- **Part XIX:** canonical addresses and pointer tagging (TBI/LAM) in the context of virtual memory (15.2 §6).
- **Part XX:** measure `noalias` benefits in loops (15.2 Systems exercise); zeroing cost vs buffer size (15.3 Systems
  exercise).
- **Part XXIII (23.5):** zero-copy formats with `zerocopy`/`bytemuck`, byte order in the type (15.2 §9).
- **Chapter 15.6 (other half):** Miri under both models in CI, intrusive linked lists (`container_of`, 15.2 §4) as the
  design exercise.

## Promises kept

- **Part XIV (14.4 §5 → 15.2):** Stacked vs Tree Borrows difference behind `crossbeam-epoch`'s `Local::element_of`,
  reduced to a minimal `container_of` listing (UB under SB, OK under TB) plus the version that passes both.
- **Part XIV:** `MaybeUninit` slots (the review's ring) explained in 15.3; `unsafe impl Send/Sync` obligations
  discussed via `MyCell` (`!Sync` inherited; what `unsafe impl Sync` would require).
- **4.1 / PROGRESS:** `split_at_mut` and `get_disjoint_mut` internals, sound versions verified under both models.
- **4.2, 4.6 → 15.2:** Stacked/Tree Borrows mechanics with real Miri reports decoded.
- **PROGRESS:** `set_len` example (correct and broken).
- **3.5, 5.3 → 15.3:** drop check, `#[may_dangle]`, and PhantomData's drop-check row, all three cases verified
  (including the unsound one without `PhantomData`).
- **2.3, 5.2 → 15.1/15.3:** validity invariants and niches under `unsafe` (`MaybeUninit`/`UnsafeCell` hide niches,
  `ManuallyDrop` keeps them).
- **2.7 → 15.1:** privacy as a memory-safety boundary (module = unit of trust).
- **1.3 → 15.1:** guarantees table, trusted base, #25860 (status updated), leakpocalypse reference.

## Meridian facts introduced

| System | Facts established | Where |
|---|---|---|
| Gateway workspace unsafe policy | `#![forbid(unsafe_code)]` in gateway-core, gateway-io, gateway bin; only `gateway-proto::headers` may contain unsafe (inline `HeaderBlock<16>` for upstream response headers, capped at 16 by proxy policy); clippy `undocumented_unsafe_blocks` + `missing_safety_doc`; `deny(unsafe_op_in_unsafe_fn)`; CODEOWNERS; Miri nightly; ~120 lines of unsafe module vs ~40,000 lines of gateway code | 15.1 §9 |
| `HeaderBlock::restore` incident | safe `restore(checkpoint)` added with no `unsafe` keyword; stale checkpoint from a pooled request context → len beyond initialized slots → allocator crashes, once a leaked header value; fix: shrink-only restore that drops; review rule changed to "PRs touching a module that contains unsafe" | 15.1 §10 |
| Market-data ingest frame parsing | C++-style raw cast proposal rejected (alignment UB + wrong byte order); safe `from_be_bytes` parser adopted for the 8-byte header; `zerocopy` derives approved for larger structures; raw byte-to-struct casts banned in the service's unsafe policy | 15.2 §9 |
| Risk-limits self-transfer incident | Rust port keeps per-merchant exposure buckets in `Vec<i64>`; hand-written `two_mut` (predates `get_disjoint_mut`) checked bounds, not distinctness; self re-route rule → two `&mut` → release reported headroom 30 lower than stored → spurious declines; found by daily reconciliation; fix: `get_disjoint_mut` + explicit `i == j` no-op, Miri in both models, ban on hand-written disjointness code | 15.2 §10 |
| Gateway pooled read buffers | 16 KiB pooled buffers; `memset` from `clear()+resize()` seen in a profile; `set_len` PR rejected (UB + ~115 ns per read ≈ 0.02% of ~500 µs CPU/request); shipped `PooledBuf` kept initialized (zeroed once), exposing only `[..filled]`; test that a short read after a long one leaks no stale bytes | 15.3 §9 |
| payments-core warm-up leak | fixed per-worker set of card-processor connections built in a `MaybeUninit` array; processor maintenance → 3rd connect failed → `expect` panic caught by the task supervisor, retried every few seconds; each attempt leaked 2 open connections (no Drop); processor's per-client connection limit filled; outage outlasted maintenance until a rolling restart; fix: drop guard, then a safe `Vec` + `?` + `try_into()` rewrite that removed the unsafe | 15.3 §10 |

## Verification

- `listings/part-15/ch01-*`, `ch02-*`, `ch03-*`: **58 files, 98 checks, all pass** on rustc 1.98.1 (edition 2024),
  final run saved to scratch `part-15/verify-final-ch01-03.txt`. 46 checks are Miri runs (7 of them `debug+tree`, Tree
  Borrows); 3 checks are `debug+nightly` (`dropck_eyepatch`).
- New listings in this pass: `ch01-19-bool-ten-twenty.rs` (answer key: prints `pick(b) = 10`; asm `test`+`cmovne`),
  `ch02-17-container-of.rs`, `ch02-18-container-of-raw.rs`, `ch02-19-alias-assumption.rs` (debug `100/100`, release
  `70/100`, Miri UB), `ch03-19-pooled-buffer.rs` (timing, three runs: 166–175 vs 287–293 ns/read; Miri-clean with a
  3-iteration `cfg(miri)` path), `ch03-20-into-raw-parts.rs`. `ch03-07-manuallydrop.rs` gained a niche size line (re-verified).
- The earlier resume writer's `ch03-01`…`ch03-18` listings had not been confirmed; all were verified in this pass.
- Artifacts via `tools/emit.ps1`: debug and release LLVM IR + release asm of `ch02-15-codegen.rs` (`inbounds nuw` vs
  plain GEP; `nth = nth_wrapping`; `body_len` = cmp/load/bswap); release asm of `transfer` (`ch02-19`, bin), `fill`
  (`ch03-18`, memset), `pick` (`ch01-19`, bin).
- Every `rust` block in 15.2 and 15.3 is a verbatim substring of a verified listing (scripted check:
  `<scratchpad>/part-15/check-blocks.ps1`); `rust,ignore` blocks are named excerpts; one block is an std excerpt
  (`MaybeUninit`'s definition), labeled as such.
- Unverifiable here and labeled: Arm/CHERI behavior, TBI/LAM, HotSpot zeroing elision (hedged), the `*mut`-parameter
  reload in the 15.2 Systems exercise answer (marked "predicted"), cache-size effects in the 15.3 Systems answer
  (order of magnitude).

## Word count

README 564 · 15.1 5,857 · 15.2 6,593 · 15.3 5,831 · answers (15.1–15.3) 4,460 → **23,305** (`wc -w`, code included).

## Tooling notes

- `+tree` works for `miri`/`miri-ok` checks; a listing can carry `debug miri Undefined` and `debug+tree miri-ok`
  together to show a Stacked-vs-Tree difference.
- Timing listings with `Read`: pass the reader as `black_box(&mut src as &mut dyn Read)` (coerce *inside*
  `black_box`) and `black_box` the result, or LLVM deletes the copy and the loop reports 0 ns.
- Print timings in ASCII units (`ns`) — `µs` comes back mis-decoded in the PowerShell console output.
- Miri lays out allocations with gaps, so "one past `a` == `&b[0]`" is `true` natively and `false` under Miri.
- `Vec::into_raw_parts` is stable on 1.98.1 (verified).
- The Playground's bumpalo (3.20.3) lacks the `collections` feature. Miri's leak report and "unsupported operation" are
  not "Undefined Behavior", so `verify.ps1`'s `miri` outcome can't check them.
- Deny-by-default lints seen: `invalid_from_utf8_unchecked`, `invalid_reference_casting`; warn: `useless_ptr_null_checks`.
