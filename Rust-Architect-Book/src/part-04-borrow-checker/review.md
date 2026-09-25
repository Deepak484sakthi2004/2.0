# Part IV Review — Borrow-Error Triage & Interview Mode

> Consolidate Part IV, then use it: triage a program with four different borrow errors, and answer senior-level
> questions without notes. Answers are in **Appendix A, Part IV**.

---

## Part IV on one page

```text
 WHAT IS CHECKED      loans on PLACES (x, x.f, *r, v[_]); a live loan restricts its place, parents, and children
                      &mut = UNIQUE, & = SHARED; mutation through & only via UnsafeCell (Cell, RefCell, Mutex, atomics)
                      UnsafeCell removes `noalias` (verified: &Cell<i32> reloads, &i32 doesn't)
        │
 WHEN A LOAN ENDS     at its LAST USE, per control-flow path (NLL, liveness)
                      reborrows are nested loans; generic T parameters MOVE a &mut (no implicit reborrow)
                      still rejected on 1.98.1: conditional return of a borrow (problem case #3) → entry API
        │
 SIGNATURES           lifetime params = regions the CALLER picks; annotations RELATE, never extend
                      elision: (1) each input its own, (2) one input → outputs, (3) &self → outputs
                      T: 'static = "owns what it needs", not "lives forever"
        │
 SUBSTITUTION         only lifetimes have subtyping; &T covariant; &mut T / Cell<T> INVARIANT; fn(T) contravariant
                      invariance of &mut is a memory-safety rule (else: write a short ref into a long slot)
        │
 CALLBACKS            for<'a> = "for EVERY lifetime": callee lends short-lived data; callback can't keep it (E0521)
        │
 ERRORS               A (loan) – B (conflict) – C (later use) = a proof; five strategies:
                      shorten · split · re-API · re-own · restructure;  unsafe to silence = UB (Miri-verified)
```

## Ten ideas to carry forward

1. **The checker reasons about places and loans**, and fields are precise but indices aren't.
2. **`&mut` means unique.** Interior mutability is the explicit, documented exception, built on `UnsafeCell`.
3. **Loans end at their last use, per path.** Scoping tricks are rarely needed since NLL.
4. **Generic parameters move references.** Reborrow explicitly, or take `&T`.
5. **The checker is conservative.** Some sound code is rejected, and the fix is usually a better API.
6. **Annotations describe relationships.** Write output lifetimes explicitly when data comes from an input, not
   `self`.
7. **`'static` bounds are satisfied by owning**, never by leaking per request.
8. **Invariance protects writes.** Keep interior mutability away from lifetime-carrying fields.
9. **Higher-ranked bounds let callers lend** without letting callbacks keep.
10. **Every error is A–B–C**, and `unsafe` that silences one deletes a proof, it doesn't fix the bug.

---

## Capstone: borrow-error triage

This session manager (listing `review-triage.rs`) produces **four** errors in one compile, verified on rustc 1.98.1:

```rust,ignore
use std::collections::HashMap;

struct Session {
    user: String,
    hits: u32,
}

struct Manager {
    sessions: HashMap<u64, Session>,
    log: Vec<String>,
}

impl Manager {
    fn touch(&mut self, id: u64) -> &Session {
        let s = self.sessions.get_mut(&id).expect("known session");
        s.hits += 1;
        self.log.push(format!("touch {id}"));
        s
    }

    fn expire_idle(&mut self) {
        for (id, s) in &self.sessions {
            if s.hits == 0 {
                self.sessions.remove(id);
            }
        }
    }

    fn rename(&mut self, id: u64, user: &str) -> &str {
        let s = self.sessions.get_mut(&id).expect("known session");
        let old = &s.user;
        s.user = user.to_string();
        old
    }
}

fn main() {
    let mut m = Manager { sessions: HashMap::new(), log: Vec::new() };
    m.sessions.insert(1, Session { user: "ada".into(), hits: 0 });
    m.sessions.insert(2, Session { user: "grace".into(), hits: 0 });

    let s = m.touch(1);
    m.expire_idle();
    println!("{} has {} hit(s)", s.user, s.hits);

    let old = m.rename(1, "ada.l");
    println!("renamed {old}; log = {:?}", m.log);
}
```

The four errors, as rustc reports them:

```text
error[E0502]: cannot borrow `self.sessions` as mutable because it is also borrowed as immutable
25 |         for (id, s) in &self.sessions {
   |                        -------------- immutable borrow occurs here / immutable borrow later used here
27 |                 self.sessions.remove(id);
   |                 ^^^^^^^^^^^^^^^^^^^^^^^^ mutable borrow occurs here

error[E0506]: cannot assign to `s.user` because it is borrowed
32 |     fn rename(&mut self, id: u64, user: &str) -> &str {
   |               - let's call the lifetime of this reference `'1`
34 |         let old = &s.user;
   |                   ------- `s.user` is borrowed here
35 |         s.user = user.to_string();
   |         ^^^^^^ `s.user` is assigned to here but it was already borrowed
36 |         old
   |         --- returning this value requires that `s.user` is borrowed for `'1`

error[E0499]: cannot borrow `m` as mutable more than once at a time
45 |     let s = m.touch(1);
   |             - first mutable borrow occurs here
46 |     m.expire_idle();
   |     ^ second mutable borrow occurs here
47 |     println!("{} has {} hit(s)", s.user, s.hits);
   |                                  ------ first borrow later used here

error[E0502]: cannot borrow `m.log` as immutable because it is also borrowed as mutable
49 |     let old = m.rename(1, "ada.l");
   |               - mutable borrow occurs here
50 |     println!("renamed {old}; log = {:?}", m.log);
   |                        ---                ^^^^^ immutable borrow occurs here
   |                        |
   |                        mutable borrow later used here
```

**Your task**, for each error:

1. Identify **A, B, and C**, and write the one-sentence ownership proof.
2. Say what would go wrong **at run time** if the compiler accepted it. (For the second error: what does `old` point to
   after the assignment?)
3. Choose a **strategy** (shorten, split, re-API, re-own, restructure) and write the fix.

Then answer three design questions:

- Why does `touch` compile, even though it pushes to `self.log` while `s` (a mutable borrow into `self.sessions`) is
  live? Which rule from Chapter 4.1 makes it legal?
- The fourth error is subtle: `m.log` is a different field from anything `rename` touches. Why does the checker still
  object? (Hint: what does the signature `fn rename(&mut self, ..) -> &str` say the result borrows from?)
- Should `touch` return `&Session` at all? What does returning a reference into the manager do to every caller?

A fixed version (`review-triage-fixed.rs`, verified) prints:

```text
session 1 has 1 hit(s); expired 1 idle session(s)
renamed ada -> ada.l; log = ["touch 1"]
```

Write your fixes first, then compare with it and with the model answers.

---

## Interview mode

*Senior-level. Answer aloud or in writing, without notes, before checking Appendix A.*

### Language

1. What does the borrow checker actually check? Answer in terms of places, loans, and accesses.
2. Why is `&mut T` better described as a *unique* reference? What does `UnsafeCell` change?
3. What is a lifetime really describing? What is it *not*?
4. What does `T: 'static` mean, and why does an owned `String` satisfy it?

### Compiler

5. How does borrow checking work conceptually? Walk through the NLL steps.
6. Why does NLL reject "problem case #3," and what does Polonius do differently?
7. How are lifetime parameters compiled? Why are they not monomorphized?
8. How does the compiler check a higher-ranked bound?

### Variance and HRTB

9. Why must `&mut T` be invariant in `T`? Give the counterexample.
10. What's the difference between `fn f<'a, F: Fn(&'a T)>` and `fn f<F: for<'a> Fn(&'a T)>`?

### Performance

11. What does `noalias` buy, and what does `Cell` cost in lost optimizations? (Use this Part's verified assembly.)
12. Do lifetimes or borrow checking cost anything at run time? What *do* they buy at run time?

### Architecture

13. When would you choose a lifetime-parameterized struct, an owned struct, or a refcounted buffer at an API boundary?
14. How would you design a zero-copy callback API that consumers can't misuse?
15. What's your team policy for borrow errors, `clone()`, `Rc<RefCell<_>>`, and `unsafe`, and why?

---

## Looking ahead: Part V

Part IV was about what the compiler can *prove about your references*. Part V turns the same machinery toward your
*domain*: types as architecture. Algebraic data types, layout and niches in depth, newtypes, zero-sized and phantom
types (where Chapter 4.4's `PhantomData` variance matters), and the **type-state pattern**, which moves invalid state
transitions, like Chapter 2.5's payment state machine, from run-time errors to compile-time errors.
