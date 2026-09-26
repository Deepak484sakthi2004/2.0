// verify: release ok
// Chapter 9.5's promise: the fraud feature vector. ~400 named features per score.
//   before: a HashMap<String, f64> built per score, weights looked up by name
//   after:  names resolved to indices once, at model load; one reused Vec<f64> per worker
// Per-score latency distribution and allocations, on 1 and on 4 threads. One Playground run, noisy.
use hdrhistogram::Histogram;
use std::collections::HashMap;
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::thread;
use std::time::Instant;

mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    pub static ALLOCS: AtomicUsize = AtomicUsize::new(0);
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
}

const FEATURES: usize = 400;

/// A stand-in for real feature extraction: cheap arithmetic on the event, different per feature.
#[inline]
fn feature(i: usize, event: u64) -> f64 {
    let x = event.wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left((i % 61) as u32) ^ i as u64;
    (x >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
}

struct Model {
    names: Vec<String>,
    weights_by_name: HashMap<String, f64>,
    weights: Vec<f64>, // same weights, in feature-index order (resolved once at load)
}

fn score_by_name(m: &Model, event: u64) -> f64 {
    let mut features: HashMap<String, f64> = HashMap::new();
    for (i, name) in m.names.iter().enumerate() {
        features.insert(name.clone(), feature(i, event));
    }
    m.weights_by_name.iter().map(|(name, w)| features[name] * w).sum()
}

fn score_by_index(m: &Model, event: u64, buf: &mut Vec<f64>) -> f64 {
    buf.clear();
    buf.extend((0..FEATURES).map(|i| feature(i, event)));
    buf.iter().zip(&m.weights).map(|(f, w)| f * w).sum()
}

fn run(model: &'static Model, threads: usize, by_index: bool) -> (Histogram<u64>, f64) {
    let scores_per_thread = 20_000u64;
    let a0 = counting::ALLOCS.load(Relaxed);
    let hs: Vec<_> = (0..threads)
        .map(|t| {
            thread::spawn(move || {
                let mut h = Histogram::<u64>::new_with_bounds(1, 1_000_000_000, 3).unwrap();
                let mut buf = Vec::with_capacity(FEATURES);
                for e in 0..scores_per_thread {
                    let event = e * 7919 + t as u64;
                    let t0 = Instant::now();
                    let s = if by_index { score_by_index(model, event, &mut buf) } else { score_by_name(model, event) };
                    h.record(t0.elapsed().as_nanos() as u64).unwrap();
                    black_box(s);
                }
                h
            })
        })
        .collect();
    let mut all = Histogram::<u64>::new_with_bounds(1, 1_000_000_000, 3).unwrap();
    for h in hs {
        all.add(h.join().unwrap()).unwrap();
    }
    let allocs = (counting::ALLOCS.load(Relaxed) - a0) as f64 / (threads as u64 * scores_per_thread) as f64;
    (all, allocs)
}

static SINK: AtomicUsize = AtomicUsize::new(0);

fn main() {
    let names: Vec<String> = (0..FEATURES).map(|i| format!("feat_{:03}_{}", i, ["velocity", "geo", "device", "amount"][i % 4])).collect();
    let weights: Vec<f64> = (0..FEATURES).map(|i| ((i * 37 % 101) as f64 - 50.0) / 100.0).collect();
    let weights_by_name = names.iter().cloned().zip(weights.iter().copied()).collect();
    let model: &'static Model = Box::leak(Box::new(Model { names, weights_by_name, weights }));

    // Same answer both ways (up to summation order).
    let mut buf = Vec::new();
    let (a, b) = (score_by_name(model, 42), score_by_index(model, 42, &mut buf));
    println!("score(42): by name {a:.6}, by index {b:.6}");

    println!("ns per score; release; one Playground run, noisy");
    println!("{:<28} {:>8} {:>8} {:>8} {:>9} {:>12}", "variant", "p50", "p99", "p99.9", "max", "allocs/score");
    for threads in [1usize, 4] {
        for by_index in [false, true] {
            let (h, allocs) = run(model, threads, by_index);
            let label = format!("{threads} thread(s), {}", if by_index { "index + reused Vec" } else { "HashMap<String,_>" });
            println!(
                "{label:<28} {:>8} {:>8} {:>8} {:>9} {:>12.1}",
                h.value_at_percentile(50.0),
                h.value_at_percentile(99.0),
                h.value_at_percentile(99.9),
                h.max(),
                allocs
            );
            SINK.fetch_add(h.len() as usize, Relaxed);
        }
    }
}
