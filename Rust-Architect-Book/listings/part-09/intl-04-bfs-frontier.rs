// verify: debug ok
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
use std::collections::VecDeque;

/// Peak frontier of BFS (queue) and DFS (explicit stack) over any graph given as a neighbor function.
/// Returns (peak BFS queue len, peak BFS queue capacity, peak DFS stack len).
fn frontiers(n: usize, start: u32, neighbors: impl Fn(u32, &mut Vec<u32>)) -> (usize, usize, usize) {
    let mut buf = Vec::new();

    let mut seen = vec![false; n];
    let mut q = VecDeque::new();
    q.push_back(start);
    seen[start as usize] = true;
    let (mut peak_q, mut peak_cap) = (1, q.capacity());
    while let Some(v) = q.pop_front() {
        buf.clear();
        neighbors(v, &mut buf);
        for &w in &buf {
            if !seen[w as usize] {
                seen[w as usize] = true;
                q.push_back(w);
            }
        }
        peak_q = peak_q.max(q.len());
        peak_cap = peak_cap.max(q.capacity());
    }

    let mut seen = vec![false; n];
    let mut s = vec![start];
    seen[start as usize] = true;
    let mut peak_s = 1;
    while let Some(v) = s.pop() {
        buf.clear();
        neighbors(v, &mut buf);
        for &w in buf.iter().rev() {
            if !seen[w as usize] {
                seen[w as usize] = true;
                s.push(w);
            }
        }
        peak_s = peak_s.max(s.len());
    }
    (peak_q, peak_cap, peak_s)
}

/// Path length from (0,0) to (side-1, side-1) on an open grid, following parent pointers.
fn grid_path(side: u32, bfs: bool) -> usize {
    let n = (side * side) as usize;
    let mut parent = vec![u32::MAX; n];
    let mut seen = vec![false; n];
    let mut frontier = VecDeque::from([0u32]);
    seen[0] = true;
    while let Some(v) = if bfs { frontier.pop_front() } else { frontier.pop_back() } {
        let (r, c) = (v / side, v % side);
        // neighbor order: right, down, left, up
        let cand = [(r, c + 1), (r + 1, c), (r, c.wrapping_sub(1)), (r.wrapping_sub(1), c)];
        for (nr, nc) in cand {
            if nr < side && nc < side {
                let w = nr * side + nc;
                if !seen[w as usize] {
                    seen[w as usize] = true;
                    parent[w as usize] = v;
                    frontier.push_back(w);
                }
            }
        }
    }
    let (mut v, mut len) = (n as u32 - 1, 0);
    while v != 0 {
        v = parent[v as usize];
        len += 1;
    }
    len
}

fn main() {
    // A complete binary tree of depth 20 (2^21 - 1 nodes), children of i are 2i+1 and 2i+2.
    let n = (1usize << 21) - 1;
    let tree = |v: u32, out: &mut Vec<u32>| {
        for c in [2 * v + 1, 2 * v + 2] {
            if (c as usize) < n {
                out.push(c);
            }
        }
    };
    let ((q, cap, s), allocs, _) = measure(|| frontiers(n, 0, tree));
    println!("binary tree, {n} nodes (depth 20):");
    println!("  BFS peak queue {q} nodes (capacity {cap} = {} KiB); DFS peak stack {s} nodes", cap * 4 / 1024);
    println!("  allocator calls for both traversals (incl. two {n}-entry visited arrays): {allocs}");

    let m = 1_000_000;
    let path = move |v: u32, out: &mut Vec<u32>| {
        if (v as usize) + 1 < m {
            out.push(v + 1);
        }
    };
    let (q, _, s) = frontiers(m, 0, path);
    println!("chain, {m} nodes: BFS peak queue {q}, DFS peak explicit stack {s} (recursion would need depth {m})");

    println!("20x20 grid, (0,0) -> (19,19): BFS path {} steps, DFS path {} steps", grid_path(20, true), grid_path(20, false));
}
