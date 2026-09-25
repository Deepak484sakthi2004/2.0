// verify: release ok

// --- instrumentation: count heap allocations and frees (GlobalAlloc is explained in Part XV) ---
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    static ALLOCS: AtomicUsize = AtomicUsize::new(0);
    static FREES: AtomicUsize = AtomicUsize::new(0);

    pub struct Counting;

    // SAFETY: both methods forward their exact arguments to `System`, which upholds the
    // GlobalAlloc contract; the counters are plain atomics, so counting never allocates.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            FREES.fetch_add(1, Relaxed);
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    #[global_allocator]
    static GLOBAL: Counting = Counting;

    pub fn measure<R>(f: impl FnOnce() -> R) -> (R, usize, usize) {
        let (a0, f0) = (ALLOCS.load(Relaxed), FREES.load(Relaxed));
        let r = f();
        (r, ALLOCS.load(Relaxed) - a0, FREES.load(Relaxed) - f0)
    }
}

use counting::measure;
use std::hint::black_box;
use std::mem::size_of;
use std::time::Instant;

/// A fee component applied to a payment amount in cents.
trait Fee {
    fn fee(&self, amount: i64) -> i64;
}

#[derive(Clone, Copy)]
struct Percent {
    bps: i64,
}
#[derive(Clone, Copy)]
struct Flat {
    cents: i64,
}
#[derive(Clone, Copy)]
struct Tiered {
    threshold: i64,
    low_bps: i64,
    high_bps: i64,
}

impl Fee for Percent {
    fn fee(&self, amount: i64) -> i64 {
        amount * self.bps / 10_000
    }
}
impl Fee for Flat {
    fn fee(&self, _amount: i64) -> i64 {
        self.cents
    }
}
impl Fee for Tiered {
    fn fee(&self, amount: i64) -> i64 {
        let bps = if amount < self.threshold { self.low_bps } else { self.high_bps };
        amount * bps / 10_000
    }
}

/// The closed-set alternative: one enum, one match.
#[derive(Clone, Copy)]
enum FeeKind {
    Percent(Percent),
    Flat(Flat),
    Tiered(Tiered),
}

impl Fee for FeeKind {
    fn fee(&self, amount: i64) -> i64 {
        match self {
            FeeKind::Percent(p) => p.fee(amount),
            FeeKind::Flat(f) => f.fee(amount),
            FeeKind::Tiered(t) => t.fee(amount),
        }
    }
}

const N: usize = 1_000_000;

/// Deterministic pseudo-random mix of the three kinds, so there is no pattern to learn.
fn kind_at(i: usize) -> FeeKind {
    let x = (i as u64).wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) >> 33;
    match x % 3 {
        0 => FeeKind::Percent(Percent { bps: 25 + (x % 8) as i64 }),
        1 => FeeKind::Flat(Flat { cents: 30 }),
        _ => FeeKind::Tiered(Tiered { threshold: 10_000, low_bps: 50, high_bps: 25 }),
    }
}

fn boxed(k: FeeKind) -> Box<dyn Fee> {
    match k {
        FeeKind::Percent(p) => Box::new(p),
        FeeKind::Flat(f) => Box::new(f),
        FeeKind::Tiered(t) => Box::new(t),
    }
}

fn sum_dyn(fees: &[Box<dyn Fee>], amount: i64) -> i64 {
    fees.iter().map(|f| f.fee(amount)).sum()
}
fn sum_enum(fees: &[FeeKind], amount: i64) -> i64 {
    fees.iter().map(|f| f.fee(amount)).sum()
}
fn sum_static<F: Fee>(fees: &[F], amount: i64) -> i64 {
    fees.iter().map(|f| f.fee(amount)).sum()
}

/// Best of `runs` timings, in nanoseconds per element.
fn best_ns(runs: usize, mut f: impl FnMut() -> i64) -> (i64, f64) {
    let (mut best, mut out) = (f64::MAX, 0);
    for _ in 0..runs {
        let t = Instant::now();
        out = black_box(f());
        best = best.min(t.elapsed().as_secs_f64());
    }
    (out, best * 1e9 / N as f64)
}

fn main() {
    let kinds: Vec<FeeKind> = (0..N).map(kind_at).collect();

    let (dyn_mixed, a_dyn, _) = measure(|| kinds.iter().map(|&k| boxed(k)).collect::<Vec<_>>());
    let (dyn_grouped, a_grp, _) = measure(|| {
        let mut v: Vec<Box<dyn Fee>> = Vec::with_capacity(N);
        for want in 0..3 {
            for &k in &kinds {
                let tag = match k { FeeKind::Percent(_) => 0, FeeKind::Flat(_) => 1, FeeKind::Tiered(_) => 2 };
                if tag == want {
                    v.push(boxed(k));
                }
            }
        }
        v
    });
    let (enums, a_enum, _) = measure(|| kinds.clone());
    let ((pct, flat, tier), a_static, _) = measure(|| {
        let n_pct = kinds.iter().filter(|k| matches!(k, FeeKind::Percent(_))).count();
        let n_flat = kinds.iter().filter(|k| matches!(k, FeeKind::Flat(_))).count();
        let (mut p, mut f, mut t) =
            (Vec::with_capacity(n_pct), Vec::with_capacity(n_flat), Vec::with_capacity(N - n_pct - n_flat));
        for &k in &kinds {
            match k {
                FeeKind::Percent(x) => p.push(x),
                FeeKind::Flat(x) => f.push(x),
                FeeKind::Tiered(x) => t.push(x),
            }
        }
        (p, f, t)
    });

    let amount = black_box(12_550i64);
    let (s1, t_dyn) = best_ns(5, || sum_dyn(black_box(&dyn_mixed), amount));
    let (s2, t_grp) = best_ns(5, || sum_dyn(black_box(&dyn_grouped), amount));
    let (s3, t_enum) = best_ns(5, || sum_enum(black_box(&enums), amount));
    let (s4, t_static) = best_ns(5, || {
        sum_static(black_box(&pct), amount) + sum_static(black_box(&flat), amount) + sum_static(black_box(&tier), amount)
    });
    assert!(s1 == s2 && s2 == s3 && s3 == s4);

    println!("{N} fees, total = {s1}");
    println!("{:<26}{:>12}{:>16}{:>10}", "design", "allocations", "slot bytes", "ns/fee");
    println!("{:<26}{:>12}{:>16}{:>10.2}", "Vec<Box<dyn Fee>> mixed", a_dyn, size_of::<Box<dyn Fee>>(), t_dyn);
    println!("{:<26}{:>12}{:>16}{:>10.2}", "Vec<Box<dyn Fee>> grouped", a_grp, size_of::<Box<dyn Fee>>(), t_grp);
    println!("{:<26}{:>12}{:>16}{:>10.2}", "Vec<FeeKind> (enum)", a_enum, size_of::<FeeKind>(), t_enum);
    println!("{:<26}{:>12}{:>16}{:>10.2}", "one Vec per type (static)", a_static, "8 / 8 / 24", t_static);
}
