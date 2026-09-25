# Part V — Types as Architecture

> **Part question:** *How do you make the compiler enforce your system's invariants, so that invalid states and invalid
> transitions become compile errors instead of production incidents, and what does that cost in bytes, instructions,
> and ergonomics?*

Parts III and IV were about what the compiler proves about *memory*: who owns a value, who may borrow it, and for how
long. Part V points the same machinery at your *domain*. A `Connection { connected: bool, authenticated: bool }` admits
a state that can't exist. A `u64` parameter accepts a tenant ID where an order ID belongs. A payment state machine that
returns `Err("invalid transition")` has an error path that exists only because the types allowed the mistake. Each of
these can move from run time to compile time, and this Part shows how, what it costs (usually nothing, and the assembly
proves it), and where it stops.

The Part pays off several earlier debts: the percent-vs-basis-points incident from Chapter 2.2, the tenant and order IDs
from the Part II and Part III reviews, `Option<u64>` being 16 bytes (Chapter 1.2), `PhantomData` variance steering
(Chapter 4.4), and Chapter 2.5's payment state machine, rebuilt so that refunding a pending payment doesn't compile.

## Chapter map

```text
5.1 Algebraic Data Types           counting values (products, sums, Option = 1 + T); invalid states unrepresentable;
                                   parse, don't validate (privacy makes it unforgeable); sentinels → enums at 0 bytes;
                                   exhaustiveness as change management (MIR: discriminant + switchInt + unreachable)
      │
5.2 Type Layout                    size, alignment, validity; default vs repr(C) (24 vs 32 bytes, verified offsets);
                                   niches counted (guaranteed vs observed); niche asm: one register vs two, and a
                                   vectorized loop with no checks; E0793 packed refs; padding refused by bytemuck;
                                   cache-line alignment against false sharing
      │
5.3 Newtypes, ZSTs, PhantomData,   Percent/BasisPoints/Cents; typed IDs with PhantomData<fn() -> T>; the PhantomData
    and Markers                    steering table (invariance and !Send verified); ZSTs (0 allocations for 1M pushes);
                                   marker traits as policy with on_unimplemented; newtype = raw fn, as an asm ALIAS
      │
5.4 The Type-State Pattern         Disconnected → Connected → Authenticated as types; the payment machine as types
                                   (E0599 for a wrong transition, E0382 for a stale one); sealed states; builders;
                                   the AnyPayment boundary for rows and events; what types can't do for payments
      │
Part V Review                      capstone: a type-driven refund workflow (states, transitions, four-eyes, layout
                                   budget, boundaries); interview mode
```

## What you'll be able to do after Part V

- Count the values a type admits, compare the count with the domain's states, and redesign until they match.
- Put parsing at system boundaries so core code never sees unvalidated data, and make the resulting types impossible to
  forge.
- Predict struct and enum sizes, including niche behavior, and know which layout facts are guaranteed.
- Keep padding and in-memory layout out of files and wire formats, and place `repr(C)` only where a boundary needs it.
- Replace primitive types with zero-cost domain types, choose the right `PhantomData` marker, and avoid the derive-bound
  trap.
- Encode state machines as types where they belong, connect them to run-time data without duplicating logic, and state
  precisely what they don't guarantee.

## Listings

`listings/part-05/`: 46 files, 48 checks, all verified on rustc 1.98.1 (edition 2024). That includes compile-error
listings for E0004, E0277, E0308, E0382, E0599, E0603, and E0793, three runs under **Miri** (two clean, one reporting the
undefined behavior of an invalid enum tag), and a `serde` boundary listing. Compiler artifacts (MIR for a `match`,
release assembly for niches, newtypes, and type-state transitions) come from `tools/emit.ps1`.
