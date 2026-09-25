// verify: debug ok
// verify: release ok
// verify: debug miri-ok
use std::error::Error;
use std::hint::black_box;
use std::mem::size_of;
use std::num::ParseIntError;

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

    /// Runs `f`, returning its result plus the allocations and frees it performed.
    pub fn measure<R>(f: impl FnOnce() -> R) -> (R, usize, usize) {
        let (a0, f0) = (ALLOCS.load(Relaxed), FREES.load(Relaxed));
        let r = f();
        (r, ALLOCS.load(Relaxed) - a0, FREES.load(Relaxed) - f0)
    }
}

#[derive(Debug)]
#[allow(dead_code)] // the field is only read through Debug
enum ConfigError {
    Invalid(ParseIntError),
}

impl From<ParseIntError> for ConfigError {
    fn from(e: ParseIntError) -> Self {
        ConfigError::Invalid(e)
    }
}

fn parse_enum(s: &str) -> Result<u32, ConfigError> {
    Ok(s.parse::<u32>()?)
}

fn parse_boxed(s: &str) -> Result<u32, Box<dyn Error + Send + Sync>> {
    Ok(s.parse::<u32>()?) // ParseIntError -> Box<dyn Error> via a blanket From impl: allocates
}

fn parse_anyhow(s: &str) -> anyhow::Result<u32> {
    Ok(s.parse::<u32>()?)
}

/// A "rich" error that carries a big context buffer inline.
#[allow(dead_code)]
struct BigError {
    context: [u8; 512],
    code: u32,
}

fn main() {
    println!("sizes: Result<u32, ConfigError>={}  Result<u32, Box<dyn Error+Send+Sync>>={}  anyhow::Result<u32>={}",
        size_of::<Result<u32, ConfigError>>(),
        size_of::<Result<u32, Box<dyn Error + Send + Sync>>>(),
        size_of::<anyhow::Result<u32>>());
    println!("sizes: Result<u64, BigError>={}  Result<u64, Box<BigError>>={}",
        size_of::<Result<u64, BigError>>(),
        size_of::<Result<u64, Box<BigError>>>());

    for input in ["8080", "80x"] {
        let (_, a, f) = counting::measure(|| drop(black_box(parse_enum(black_box(input)))));
        let (_, b, g) = counting::measure(|| drop(black_box(parse_boxed(black_box(input)))));
        let (_, c, h) = counting::measure(|| drop(black_box(parse_anyhow(black_box(input)))));
        println!("{input:>5}: enum {a} alloc/{f} free   Box<dyn Error> {b}/{g}   anyhow {c}/{h}");
    }
}
