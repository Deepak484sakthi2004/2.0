# Part VI — Traits

> **Part question:** *How does one contract serve both compile-time and run-time polymorphism, what does each cost
> down to the instruction, and how do you decide, per call site, which one a system should use?*

Traits are where Rust's abstractions meet its performance model. The same `trait RateLimiter` can be used through a
generic bound, compiled per type and inlined into a loop, or through `dyn RateLimiter`, a fat pointer and a vtable call.
The *caller* chooses. Java makes that choice for you (every interface call is virtual, and the JIT speculates its way
back to static), and C++ splits it across two unrelated mechanisms (templates and virtual functions). Rust puts both
behind one contract and makes you say which one you mean.

This Part builds the contract first: bounds, default methods, method resolution through `Deref`, associated types
versus generic parameters, blanket impls. Then it establishes the rule that makes Rust's traits trustworthy at scale,
**coherence**: one impl per trait and type in the whole program, enforced crate by crate by the orphan rule. Then it
opens a trait object, with the vtable read out of real LLVM IR and a dynamic call read out of real assembly, and derives
every dyn-compatibility rule from what a vtable can hold. It ends with the decision the Part is named for, **static vs
dynamic dispatch**, measured, explained from the generated code, and turned into a decision matrix covering compile
time, binary size, dispatch cost, cache locality, extensibility, ABI, and plugin architectures.

## Chapter map

```text
6.1 Traits, Bounds, Default Methods   contracts separate from types; three spellings of a bound; generic code checked at
                                      the definition (E0369); method resolution through Deref (Tracked<T> counts it);
                                      AsRef/Into with measured allocations; the default method that caused a retry storm
      │
6.2 Associated Types, Generic         "who chooses the type?": outputs vs inputs; From/Into and Display/ToString as
    Traits, Blanket Impls             blanket impls; supertraits; the blanket impl that broke payments (E0119)
      │
6.3 Coherence and the Orphan Rule     one impl per (trait, type), program-wide; the precise orphan rule (fundamental types,
                                      dyn LocalTrait, local type parameters); "upstream crates may add a new impl";
                                      newtype vs extension trait vs upstream feature; why impls live with trait or type
      │
6.4 Trait Objects, vtables, and       fat pointers (all 16 bytes, measured); the vtable in LLVM IR (null drop entry for
    dyn Compatibility                 types without drop glue); `call qword ptr [rax + 24]` in assembly; dyn-compatibility
                                      rules derived from the vtable (E0038); Send/Sync on objects; Any; variance
      │
6.5 Static vs Dynamic Dispatch        measured: 3.3 / 2.0 / 1.0 / 0.8 ns per element for mixed dyn, grouped dyn, enum,
                                      static; explained from assembly; the ratio test; the seven-dimension decision matrix;
                                      why a plugin boundary needs a C ABI (hand-built vtable, clean under Miri)
      │
Part VI Review                        review a fraud-rules SDK with an orphan impl, a dyn-incompatible trait, and a silent
                                      default; redesign it; interview mode
```

## What you'll be able to do after Part VI

- Design traits whose defaults are safe, whose type members are associated or generic for the right reason, and whose
  blanket impls don't lock out the impls users will need.
- Predict the verdict of the orphan and overlap checks, and choose among newtypes, extension traits, and upstream
  features when they say no.
- Read a trait object at the machine level: fat pointer, vtable layout, and the instructions of a dynamic call.
- Keep a trait dyn compatible on purpose, with `where Self: Sized`, extension traits, `clone_box`, and a compile-time
  guard.
- Choose generics, enums, or `dyn` per call site with a decision matrix and a ratio test, not folklore, and know which
  choices need a C ABI.

## Listings

`listings/part-06/`: 41 files, 43 checks, all verified on rustc 1.98.1 (edition 2024), including one run under **Miri**
(the hand-built C-ABI vtable). Compiler artifacts (LLVM IR for vtables, x86-64 assembly for static, enum, and dynamic
dispatch) were fetched with `tools/emit.ps1`. Timings come from Playground release builds (four runs, noisy, and
labeled as such).
