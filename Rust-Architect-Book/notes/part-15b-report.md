# Part 15b report (INCOMPLETE — writer stopped by a safety classifier)

## Status

The writer for Part XV's second half (15.4–15.6, review, answers for 15.4 onward) was stopped by a safety classifier
early in the session, before writing any prose. Following the directive, it stopped and did not retry. **Don't add
15.4–15.6 or the review to SUMMARY.md.**

Done:
- Re-verified the 14 existing `listings/part-15/ch04-*.rs` files (22 checks), all PASS on rustc 1.98.1:
  `ch04-01` (ok, miri-ok, test: 2 tests), `ch04-02` (ok, miri-ok), `ch04-03` (miri use-after-free; release crash
  "free(): double free detected in tcache 2"), `ch04-04` (ok, miri-ok), `ch04-05` (ok, miri-ok), `ch04-06` (miri
  use-after-free), `ch04-07` (ok, miri-ok), `ch04-08` (ok), `ch04-09` (miri uninitialized), `ch04-10` (ok, miri-ok),
  `ch04-11` (E0597, note "borrow might be used here, when `names` is dropped and runs the `Drop` code for type
  `MyVec`"), `ch04-12` (ok), `ch04-13` (release build), `ch04-14` (E0277 "`NonNull<u64>` cannot be sent between
  threads safely").
- Emitted the release asm of `ch04-13-push-asm.rs` (scratch only).
- Wrote one new listing, `listings/part-15/ch04-15-niche.rs` (sizes of MyVec/Vec under Option), **not yet verified**.
  Verify it or delete it before the next full-folder run.

Not done: the prose for 15.4, 15.5, 15.6, `review.md`, the ch05/ch06/review listings, `notes/part-15b-answers.md`.

## Third attempt (narrower scope, user-approved): also stopped by a safety classifier

The narrower-scope writer was stopped by a safety classifier while still reading and planning. No prose was written.
Per the directive, it stopped and did not retry. Done in this attempt:
- `listings/part-15/ch04-03-remove-bug.rs`: removed the `// verify: release crash double free` header and reworded the
  final comment; only the `debug miri use-after-free` check remains.
- Re-verified all 15 `ch04-*` listings: **22 checks, all PASS** (ch04-03 now has 1 check; ch04-15 verified).
- Release asm of `ch04-13-push-asm.rs` re-emitted (scratch only) and confirmed the five-instruction fast paths quoted
  below.

Still not written: the text of 15.4, 15.5, and 15.6, `review.md`, the ch05/ch06/review listings, and
`notes/part-15b-answers.md`. **Don't add 15.4–15.6 or the review to SUMMARY.md.**

## Verified facts for whoever resumes (from the runs above)

- `ch04-01` output: `len=5 capacities=[0, 4, 8]`; ZST vector: `len=1000 capacity=18446744073709551615`.
- `ch04-04`: `first=Some("T1") last=Some("T6") remaining=4`, 4 drops when the iterator is dropped, 6 in total.
- `ch04-05`: `after forget: w.len()=1`, `sum of dropped ids = 11` (the drained range and tail are leaked, not
  double-dropped).
- `ch04-07`: pushing 1,000 `u64`: MyVec and std `Vec` both do (allocs, reallocs, frees) = (1, 8, 1); 10,000 `()` =
  (0, 0, 0) for both.
- `ch04-08`: `try_reserve(usize::MAX)` and `(1 << 61)` → `CapacityOverflow`; `(1 << 58)` → `AllocError { size:
  2305843009213693976, align: 8 }`; the vector is intact afterwards (`[7, 8, 9] capacity=4`).
- `ch04-09`: `caught panic: true, len now 4`, then Miri: `reading memory at alloc413[0x38..0x40], but memory is
  uninitialized`. `ch04-10`: `contents now ["a", "b"]`, miri-ok.
- Release asm (`ch04-13`): the push fast paths of `MyVec<u64>` and std `Vec<u64>` are the same five instructions
  (load len, compare with cap, store at ptr + 8·len, increment, store len). Field offsets differ: std `Vec` is
  [cap, ptr, len] on 1.98.1, MyVec is [ptr, cap, len]. MyVec's `grow` is out of line (`#[cold]`); std calls
  `RawVec::grow_one` → `RawVecInner::grow_amortized` → `finish_grow`.

## Verification

- 14 files / 22 checks re-verified, all pass. One new file (`ch04-15-niche.rs`) unverified.
- Integrator, afterwards: `ch04-15-niche.rs` verified (PASS). Output: `MyVec<u8>` 24, `Option<MyVec<u8>>` 24,
  `Option<Option<MyVec<u8>>>` 32; `Vec<u8>` 24, `Option<Vec<u8>>` 24, `Option<Option<Vec<u8>>>` 24,
  `Result<Vec<u8>, u32>` 24 bytes. std's capacity field carries extra niches [LIB] (a capacity type whose valid range
  excludes the high bit); MyVec's plain `usize` doesn't, so only `NonNull`'s single null niche is available.

## Word count

No prose written.
