// verify: release ok
// Listing 17.4-2: string interning. Every identifier occurrence becomes a 4-byte Symbol; each
// distinct string is stored once. Name comparisons become integer comparisons, and symbol tables
// hash a u32 instead of a string. (rustc's `Symbol` is the same idea: a u32 into a global interner.)
// Allocations are exact; timings are best-of-5 in release, one Playground run: noisy.

mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    static ALLOCS: AtomicUsize = AtomicUsize::new(0);

    pub struct Counting;

    // SAFETY: both methods forward their exact arguments to `System`, which upholds the
    // GlobalAlloc contract; the counter is a plain atomic, so counting never allocates.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    #[global_allocator]
    static GLOBAL: Counting = Counting;

    pub fn allocs() -> usize {
        ALLOCS.load(Relaxed)
    }
}

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Symbol(u32);

/// Each distinct string is allocated once (an `Rc<str>` shared by the map and the vector).
#[derive(Default)]
pub struct Interner {
    map: HashMap<Rc<str>, Symbol>,
    strings: Vec<Rc<str>>,
}

impl Interner {
    pub fn intern(&mut self, s: &str) -> Symbol {
        if let Some(&sym) = self.map.get(s) {
            return sym; // hit: no allocation
        }
        let sym = Symbol(self.strings.len() as u32);
        let rc: Rc<str> = Rc::from(s);
        self.strings.push(rc.clone());
        self.map.insert(rc, sym);
        sym
    }
    pub fn resolve(&self, sym: Symbol) -> &str {
        &self.strings[sym.0 as usize]
    }
}

fn identifiers(src: &str) -> impl Iterator<Item = &str> {
    src.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| !w.is_empty() && !w.as_bytes()[0].is_ascii_digit())
}

fn best<R>(mut f: impl FnMut() -> R) -> f64 {
    (0..5).map(|_| { let t = Instant::now(); std::hint::black_box(f()); t.elapsed().as_secs_f64() })
          .fold(f64::MAX, f64::min)
}

fn main() {
    // A synthetic program: 1,000 distinct local names, used over and over.
    let mut src = String::new();
    for i in 0..40_000 {
        let (a, b, c) = (i % 1000, (i * 7) % 1000, (i * 13) % 1000);
        src.push_str(&format!("let value_{a} = value_{b} + compute_total_{c}(value_{a});\n"));
    }
    let occurrences = identifiers(&src).count();

    // Owned strings: one allocation per occurrence.
    let a0 = counting::allocs();
    let owned: Vec<String> = identifiers(&src).map(str::to_string).collect();
    let owned_allocs = counting::allocs() - a0;

    // Interned: one allocation per DISTINCT name (plus table growth).
    let a0 = counting::allocs();
    let mut interner = Interner::default();
    let syms: Vec<Symbol> = identifiers(&src).map(|s| interner.intern(s)).collect();
    let interned_allocs = counting::allocs() - a0;

    println!("{occurrences} identifier occurrences, {} distinct", interner.strings.len());
    println!("  Vec<String>: {owned_allocs:>7} allocations, {} B per handle", size_of::<String>());
    println!("  Vec<Symbol>: {interned_allocs:>7} allocations, {} B per handle", size_of::<Symbol>());
    println!("  round trip: {:?} -> {:?}", syms[0], interner.resolve(syms[0]));

    // A later pass: look every use up in a symbol table (name -> declaration index).
    let by_string: HashMap<String, usize> = owned.iter().cloned().enumerate().map(|(i, s)| (s, i)).collect();
    let by_symbol: HashMap<Symbol, usize> = syms.iter().copied().enumerate().map(|(i, s)| (s, i)).collect();
    let t_str = best(|| owned.iter().map(|s| by_string[s]).sum::<usize>());
    let t_sym = best(|| syms.iter().map(|s| by_symbol[s]).sum::<usize>());
    let eq_str = best(|| owned.windows(2).filter(|w| w[0] == w[1]).count());
    let eq_sym = best(|| syms.windows(2).filter(|w| w[0] == w[1]).count());
    println!("lookups (release, best of 5, one run): String keys {:.2} ms, Symbol keys {:.2} ms",
        t_str * 1e3, t_sym * 1e3);
    println!("equality scans:                          String {:.2} ms, Symbol {:.2} ms",
        eq_str * 1e3, eq_sym * 1e3);
}
