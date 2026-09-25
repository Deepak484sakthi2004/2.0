# Part III Review — Architecture Review & Interview Mode

> Consolidate Part III, then use it: redesign a Java-shaped data structure around ownership, and answer senior-level
> questions without notes. Answers are in **Appendix A, Part III**.

---

## Part III on one page

```text
 OWNERSHIP        every value has ONE owner; ownership forms a TREE; drop the root → drop the subtree
   │              compiler: drop obligations → drop flags (only when control flow decides) → drop glue
   │              cost of dropping = the SHAPE of the tree (Vec<u64>: 1 free; Vec<String>: 1,001)
   ▼
 MOVE             copy size_of::<T>() bytes + a compile-time death certificate for the source
   │              24 bytes for a String however long; 4 KiB for a [u8; 4096]  → Box large inline values
   │              Copy = a bitwise copy IS a full duplicate (so no Drop);  Clone = explicit, arbitrary cost
   ▼
 BORROW           &T: many readers; &mut T: one writer; never both  (aliasing XOR mutation)
   │              references never outlive the referent; a borrowed value may not MOVE
   │              payoff: memory safety + data-race freedom + noalias (verified: `*b` loaded once)
   │              mirrors the CPU's single-writer / multiple-reader cache-coherence invariant
   ▼
 SLICES & TEXT    owned/borrowed pairs (String/&str, Vec/&[T], PathBuf/&Path); fat pointers; UTF-8 always
   │              bytes ≠ chars ≠ graphemes ≠ display width; slice only at boundaries
   │              Cow: borrow when you can, own when you must (measured: allocate only on change)
   ▼
 DROP / RAII      scope end (reverse order), fields in order, temporaries at end of statement;
   │              unwinding runs cleanup blocks; exit/abort/forget/cycles skip them
   │              Drop can't fail, can't await, shouldn't block → explicit fallible close + Drop backstop
   ▼
 GRAPHS           Rc/Weak (shared, counted) · RefCell (checks move to RUN time) · arenas + indices
                  · generational keys (stale handles detected) · index-linked lists (an LRU, no unsafe)
```

## Ten ideas to carry forward

1. **Ownership is a tree, and a Rust data design starts by drawing it.** Everything else is an explicit exception.
2. **"Exactly once" costs nothing at run time.** Drop flags exist only where control flow decides, and vanish in
   release.
3. **Dropping is work on the dropping thread.** Design for large drops: dropper threads, arenas.
4. **A move is a small copy plus a compile-time fact.** Its cost is `size_of::<T>()`, not the size of what it owns.
5. **A clone is a design question.** In hot paths, measure it. Borrow or `Arc` instead.
6. **Aliasing XOR mutation buys three things at once:** memory safety, data-race freedom, and optimizations C can't do.
7. **Borrowing pins a value's address.** A borrowed value can't move, and a borrowed collection can't reallocate.
8. **Text has four lengths.** Choose one per layer and name it.
9. **`Drop` is a `finally` that can't throw.** Pair it with explicit fallible operations for anything whose failure
   matters.
10. **`Rc<RefCell<T>>` is Java's object model re-created at run time.** An arena with indices is usually the Rust
    answer.

---

## Architecture review: a Java-shaped order book

A team porting Meridian's matching-engine prototype from Java submits this core data structure (listing
`review-order-book.rs`; it compiles):

```rust,ignore
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone)]
pub struct Order {
    pub id: u64,
    pub price_cents: i64,
    pub qty: u32,
    pub book: Option<Rc<RefCell<OrderBook>>>, // back-reference "for convenience"
}

pub struct OrderBook {
    pub bids: Vec<Rc<RefCell<Order>>>,
    pub asks: Vec<Rc<RefCell<Order>>>,
    pub by_id: HashMap<u64, Rc<RefCell<Order>>>,
    pub history: Vec<Order>, // a snapshot of every order ever placed
}

impl OrderBook {
    pub fn place(book: &Rc<RefCell<OrderBook>>, mut order: Order, is_bid: bool) {
        order.book = Some(Rc::clone(book));
        let shared = Rc::new(RefCell::new(order.clone()));
        let mut b = book.borrow_mut();
        b.history.push(order);
        b.by_id.insert(shared.borrow().id, Rc::clone(&shared));
        if is_bid {
            b.bids.push(shared);
        } else {
            b.asks.push(shared);
        }
        b.bids.sort_by(|x, y| y.borrow().price_cents.cmp(&x.borrow().price_cents));
    }

    pub fn cancel(&mut self, id: u64) {
        self.by_id.remove(&id);
    }
}
```

**Part 1: find the problems.** There are at least ten, all about ownership, and each one maps to a chapter in this
Part. For each, write **location → problem → consequence → fix.** Hints: draw the ownership graph (including what
`history` and `order.clone()` hold); ask who owns an order after `cancel`; ask what happens to memory after a million
orders; ask what happens when the engine goes multi-threaded (Part XI); ask what `place` costs as the book grows.

**Part 2: redesign it.** Propose the ownership structure for a production order book:

- Who owns each order? What do `bids`, `asks`, and `by_id` hold instead of `Rc<RefCell<Order>>`?
- How do operations reach the book without a back-reference?
- What replaces `history`? (Hint: an append-only log of small events, not clones of live objects.)
- How do you get price-time priority without re-sorting on every insert? (Part IX covers `BTreeMap` and `VecDeque`.)

Answer Part 2 with type definitions and one-line justifications, not full code. The model answer is in Appendix A.

---

## Interview mode

*Senior-level. Answer aloud or in writing, without notes, before checking Appendix A.*

### Language

1. Why does Rust have ownership? Answer in terms of the three memory-safety bug classes it removes, and what it costs.
2. What's the difference between `Copy` and `Clone`? Why is `&T` `Copy` and `&mut T` not?
3. What is a lifetime really describing? (Use the "loan with a deadline" picture from Chapter 3.3; Part IV formalizes
   it.)
4. Why are Rust references never null, and what's the equivalent of a nullable reference?

### Compiler

5. How does the compiler guarantee a value is dropped exactly once when moves are conditional?
6. What is drop glue, and which types don't have any?
7. What does `noalias` on a `&mut` parameter let LLVM do? Give the `add_twice` example from memory.
8. What does the MIR of a function with two `String` locals look like on its unwind path?

### Performance

9. When does `clone()` become expensive? Give numbers for a `Vec<String>` of 1,000 elements.
10. What does moving a value cost, and when is that cost significant?
11. Why can dropping a large structure cause a latency spike, and how do you avoid it?
12. When does `Cow` pay off, and what does it cost?

### Architecture

13. How would you model a graph with frequent deletions and outside handles in Rust? Compare three options.
14. Why can't `Drop` do a network call reliably, and how would you design leases or locks around that?
15. A team's Rust port is full of `Rc<RefCell<_>>` and `.clone()`. What does that tell you, and what do you recommend?

---

## Looking ahead: Part IV

Part III used the borrow checker's *verdicts*. Part IV opens the checker: aliasing XOR mutation as the formal rule,
non-lexical lifetimes and liveness, reborrowing, lifetime annotations and elision (finally explaining `longest<'a>`,
`Record<'a>`, and `Tx<'db>`), variance, higher-ranked trait bounds, and the skill this whole book has been building
toward: **reading any borrow-checker error as an ownership proof.**
