# Part X Review — The Settlement Report PR & Interview Mode

> Consolidate Part X, then use it: review an iterator-heavy PR that compiles, runs, and prints plausible numbers, and
> answer senior-level questions without notes. Answers are in **Appendix A, Part X**.

---

## Part X on one page

```text
 CLOSURES        anonymous struct of captures + Fn* impls; size = captures (0 B .. whatever you move in)
                 capture mode per PLACE, inferred (& → &mut → by value); `move` = everything by value (Copy = copy!)
                 trait from the BODY: reads → Fn(&self), mutates → FnMut(&mut self), moves out → FnOnce(self)
                 callers: ask for the weakest trait you call; no two closures share a type
        │
 CALLING STYLES  generic F: Fn → inlined (multiply-shift, unrolled);  &dyn Fn / fn ptr → call per element
                 vtable for dyn Fn: [drop, size, align, call_once, call_mut, call]   (verified asm)
        │
 ITERATOR        next() -> Option<Item>; lazy, pull-based, one item through the whole pipeline at a time
                 iter() &T · iter_mut() &mut T · into_iter() T;  None may not be final (FusedIterator)
                 size_hint is a hint; errors belong IN the item type; Item can't borrow from &mut self → GATs
        │
 COMPILATION     adapters are structs (the type IS the pipeline) + monomorphization + inlining
                 + internal iteration (fold/try_fold: Chain = two vectorized loops; for-loop = one checked loop)
                 dyn Iterator: only next() crosses → ~20× on tight loops; debug builds: 10–40× slower
                 collect() mid-pipeline allocates; into_iter().filter().collect() may keep the SOURCE capacity
        │
 VS STREAMS      same vocabulary; different defaults: no boxing, stack pipeline, AOT optimization,
                 errors as data (collect::<Result>, try_fold), HashMap collect keeps the LAST duplicate,
                 reuse = compile error (clone borrowing iterators), parallel f64 sums vary run to run
```

## Ten ideas to carry forward

1. **A closure is a struct you didn't have to write.** Its size, its `Send`/`Sync`, and its `Copy`-ness all come from
   its captures.
2. **`move` decides how captures are taken, not what the closure can do.** A `move` closure over a `Copy` counter
   counts privately.
3. **Accept the weakest `Fn*` trait you need, and return `impl Fn` for one closure or `Box<dyn Fn>` for a choice.**
4. **Dynamic dispatch costs what it prevents** (inlining, vectorization), far more than the `call` instruction.
5. **Iterators are lazy and vertical**: build freely, consume deliberately, and remember that short-circuiting
   consumers skip side effects.
6. **Own, borrow, or mutate by choosing `into_iter`, `iter`, or `iter_mut`**, and consume a collection when the loop is
   its last user.
7. **Put errors in the item type**, and let the consumer choose the policy.
8. **Prefer `fold`-based consumers for `chain`/`flatten`**, keep `dyn` at coarse boundaries, and never time a debug
   build.
9. **`len` is not memory.** In-place collect and `retain` keep capacity, and `shrink_to_fit` gives it back.
10. **Port Java streams by semantics, not by syntax.** `toMap` fails on duplicates, `collect` doesn't, and a test that
    only feeds `Vec`s can hide a `zip` that drops items.

---

## Capstone: the settlement report PR

A PR adds the nightly per-merchant settlement summary to Meridian's settlement batch job (Chapter 10.3). It computes
fees and payouts for settled EUR transactions, records every settled transaction for compliance screening, keeps the
settled set for the ops dashboard, and sends payouts to the bank in batches of 5 (the bank's per-request limit). CI is
green: it compiles with two warnings, and the job prints plausible numbers.

Listing `review-settlement.rs` (verified; sample data and `main` abridged here):

```rust,ignore
/// Parses "merchant,bps" rows from the partner's fee-schedule export.
fn load_fees(rows: &[&str]) -> HashMap<String, u32> {
    rows.iter()
        .map(|r| {
            let (m, bps) = r.split_once(',').unwrap();
            (m.to_string(), bps.trim().parse().unwrap())
        })
        .collect()
}

fn build_report(txns: Vec<Txn>, fees: &HashMap<String, u32>) -> Report {
    let mut skipped_non_eur = 0;
    let settled: Vec<Txn> = txns.into_iter().filter(|t| t.status == Status::Settled).collect();
    let eur: Vec<&Txn> = settled
        .iter()
        .filter(move |t| {
            if t.currency != "EUR" {
                skipped_non_eur += 1;
                return false;
            }
            true
        })
        .collect();

    let mut by_merchant: HashMap<String, Vec<&Txn>> = HashMap::new();
    for t in eur.iter() {
        by_merchant.entry(t.merchant.clone()).or_default().push(t);
    }
    let mut fees_cents = 0;
    let payouts: Vec<Payout> = by_merchant
        .into_iter()
        .map(|(m, ts)| {
            let bps = fees[&m] as f64;
            let gross: u64 = ts.iter().map(|t| t.cents).sum();
            let fee: u64 = ts.iter().map(|t| (t.cents as f64 * bps / 10_000.0) as u64).sum();
            fees_cents += fee;
            Payout { merchant: m, cents: gross - fee }
        })
        .collect();

    let mut audited = Vec::new();
    let _suspicious = settled.iter().map(|t| {
        audited.push(t.id);
        t
    })
    .any(|t| t.cents > 1_000_000);

    Report { settled, payouts, fees_cents, skipped_non_eur, audited }
}

/// Sends payouts to the bank API in batches of `size` (the bank's per-request limit).
fn send_in_batches(payouts: &mut impl Iterator<Item = Payout>, size: usize) -> Vec<Vec<Payout>> {
    let mut sent = Vec::new();
    loop {
        let batch: Vec<Payout> = payouts.by_ref().zip(0..size).map(|(p, _)| p).collect();
        if batch.is_empty() {
            return sent;
        }
        sent.push(batch);
    }
}
```

`main` builds 100,000 transactions across 12 merchants (`m-100` … `m-1200`), loads a 13-row fee export in which the
partner repeated `m-100` with a promotional rate, runs the report, sends the payouts through a channel (as production
does), and prints:

```text
fee for m-100:          190 bps
settled kept:           len 50000 / capacity 100000 (6 MB held)
skipped non-EUR:        0 (actual 7144)
audited:                4 of 50000 settled
fees charged:           28948189 cents
payout batches sent:    2 batches, 10 payouts (merchants with EUR volume: 12)
```

And the compiler said, in the CI log nobody read:

```text
warning: value captured by `skipped_non_eur` is never read
  --> src/main.rs:53:17
   |
53 |                 skipped_non_eur += 1;
   |                 ^^^^^^^^^^^^^^^
   |
   = help: did you mean to capture by reference instead?
```

**Your task.**

1. Find **at least ten** defects. For each, name the Part X mechanism (or earlier chapter) behind it, and say whether
   the output above already shows it or it would show up later.
2. Rank them by business impact: money, compliance, data loss, memory, performance.
3. Write the fixed design. Listing `review-settlement-fixed.rs` is one defensible fix, with 5 tests. It runs as:

```text
partner export with duplicate: Err(Duplicate("m-100"))
settled 50000 / audited 50000 / suspicious [5]
skipped non-EUR: 7144
fees: integer half-up 30397321 cents vs f64 truncation 30375887 cents
payout batches: [5, 5, 2]
```

4. Two design questions. The fixed version replaces most of the iterator chains in `build_report` with a `for` loop
   over a filtered iterator. When is that the better choice, and when is it a regression? And should the report keep
   `settled: Vec<Txn>` for the dashboard at all?

---

## Interview mode

Answer aloud, in two minutes each, without notes.

### Language

1. What is a closure's type, and what decides its size? What does `move` change?
2. Explain `Fn`, `FnMut`, and `FnOnce` through their receivers. Which bound should a function that calls a callback in
   a loop require?
3. Why does `impl Fn` in return position accept only one closure, and what are the alternatives?
4. Explain `for x in v`, `for x in &v`, and `for x in &mut v` in terms of `IntoIterator`.

### Compiler

5. Walk through how `v.iter().filter(p).map(f).sum()` becomes a single loop. What role does each of
   monomorphization, inlining, and internal iteration play?
6. What does the MIR of a closure look like, and how are its captured variables accessed?
7. Why can't a std `Iterator` lend references into its own buffer? How do GATs change that?

### Performance

8. Where does "zero-cost" fail for iterators? Give four shapes, with the fix for each.
9. What does `dyn Iterator` cost on a tight loop, and why is the `call` instruction not the main cost?
10. What is in-place `collect`, and how can it increase a service's memory use?

### Java

11. Compare Java lambdas with Rust closures: representation, capture, allocation, and dispatch.
12. Compare Java streams with Rust iterators: evaluation model, reuse, errors, and boxing.
13. Name three Java stream idioms whose literal Rust translation changes behavior.

### Architecture

14. How would you design a configuration-driven rule engine whose rules change at run time without a deploy?
15. What's your team's policy for iterator code on hot paths, and what measurement would make you change it?

---

## Looking ahead: Part XI

Part X kept everything on one thread. Closures that capture by reference, iterators that borrow collections, and
`FnMut` callbacks that mutate state were all proven safe by the borrow checker *within one thread of control*. Part XI
asks what happens when a closure is handed to **another thread**. That's where `move` becomes mandatory, `'static`
becomes the default bound, `Send` and `Sync` decide which captures are allowed, and `Fn` versus `FnMut` becomes the
difference between "shareable" and "needs a lock." Rayon's `par_iter` from Chapter 10.4 is where the two Parts meet.
Chapter 11.6 shows how its work-stealing scheduler splits an iterator, and why the closures you pass it must be `Fn +
Send + Sync`.
