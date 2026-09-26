// verify: release ok
// Listing 17.3-6: where AST nodes live. One generic parser, three node stores:
//   Box   - every child is its own heap allocation (the textbook enum)
//   arena - all nodes in one Vec, children are u32 indices
//   bump  - nodes in a bumpalo arena, children are &'bump references
// Allocations and frees are exact (counting allocator); times are best-of-5, one run: noisy.

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

    pub fn snapshot() -> (usize, usize) {
        (ALLOCS.load(Relaxed), FREES.load(Relaxed))
    }
}

use std::time::Instant;

/// What the parser needs from a node store.
trait Builder {
    type Node;
    fn num(&mut self, n: i64) -> Self::Node;
    fn var(&mut self, slot: u8) -> Self::Node;
    fn bin(&mut self, op: u8, a: Self::Node, b: Self::Node) -> Self::Node;
}

/// expr := term (('+'|'-') term)* ; term := atom (('*'|'/') atom)* ; atom := NUM | VAR | '(' expr ')'
struct P<'s> { s: &'s [u8], i: usize }

impl P<'_> {
    fn skip(&mut self) {
        while self.i < self.s.len() && self.s[self.i] == b' ' { self.i += 1; }
    }
    fn expr<B: Builder>(&mut self, b: &mut B) -> B::Node {
        let mut lhs = self.term(b);
        loop {
            self.skip();
            match self.s.get(self.i) {
                Some(&op) if matches!(op, b'+' | b'-') => { self.i += 1; let r = self.term(b); lhs = b.bin(op, lhs, r); }
                _ => return lhs,
            }
        }
    }
    fn term<B: Builder>(&mut self, b: &mut B) -> B::Node {
        let mut lhs = self.atom(b);
        loop {
            self.skip();
            match self.s.get(self.i) {
                Some(&op) if matches!(op, b'*' | b'/') => { self.i += 1; let r = self.atom(b); lhs = b.bin(op, lhs, r); }
                _ => return lhs,
            }
        }
    }
    fn atom<B: Builder>(&mut self, b: &mut B) -> B::Node {
        self.skip();
        let c = self.s[self.i];
        self.i += 1;
        match c {
            b'(' => { let e = self.expr(b); self.skip(); self.i += 1; e }
            b'a'..=b'z' => b.var(c - b'a'),
            _ => {
                let mut n = (c - b'0') as i64;
                while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                    n = n * 10 + (self.s[self.i] - b'0') as i64;
                    self.i += 1;
                }
                b.num(n)
            }
        }
    }
}

fn apply(op: u8, a: i64, b: i64) -> i64 {
    match op {
        b'+' => a.wrapping_add(b),
        b'-' => a.wrapping_sub(b),
        b'*' => a.wrapping_mul(b),
        _ => if b == 0 { 0 } else { a.wrapping_div(b) },
    }
}

// ---- store 1: Box ----
enum BoxExpr { Num(i64), Var(u8), Bin(u8, Box<BoxExpr>, Box<BoxExpr>) }
struct BoxB;
impl Builder for BoxB {
    type Node = Box<BoxExpr>;
    fn num(&mut self, n: i64) -> Box<BoxExpr> { Box::new(BoxExpr::Num(n)) }
    fn var(&mut self, s: u8) -> Box<BoxExpr> { Box::new(BoxExpr::Var(s)) }
    fn bin(&mut self, op: u8, a: Box<BoxExpr>, b: Box<BoxExpr>) -> Box<BoxExpr> { Box::new(BoxExpr::Bin(op, a, b)) }
}
fn eval_box(e: &BoxExpr, env: &[i64]) -> i64 {
    match e {
        BoxExpr::Num(n) => *n,
        BoxExpr::Var(s) => env[*s as usize],
        BoxExpr::Bin(op, a, b) => apply(*op, eval_box(a, env), eval_box(b, env)),
    }
}

// ---- store 2: Vec arena with u32 indices ----
#[derive(Clone, Copy)]
enum Node { Num(i64), Var(u8), Bin(u8, u32, u32) }
struct Arena { nodes: Vec<Node> }
impl Builder for Arena {
    type Node = u32;
    fn num(&mut self, n: i64) -> u32 { self.nodes.push(Node::Num(n)); self.nodes.len() as u32 - 1 }
    fn var(&mut self, s: u8) -> u32 { self.nodes.push(Node::Var(s)); self.nodes.len() as u32 - 1 }
    fn bin(&mut self, op: u8, a: u32, b: u32) -> u32 { self.nodes.push(Node::Bin(op, a, b)); self.nodes.len() as u32 - 1 }
}
fn eval_arena(nodes: &[Node], id: u32, env: &[i64]) -> i64 {
    match nodes[id as usize] {
        Node::Num(n) => n,
        Node::Var(s) => env[s as usize],
        Node::Bin(op, a, b) => apply(op, eval_arena(nodes, a, env), eval_arena(nodes, b, env)),
    }
}

// ---- store 3: bumpalo ----
enum BumpExpr<'b> { Num(i64), Var(u8), Bin(u8, &'b BumpExpr<'b>, &'b BumpExpr<'b>) }
struct BumpB<'b> { bump: &'b bumpalo::Bump }
impl<'b> Builder for BumpB<'b> {
    type Node = &'b BumpExpr<'b>;
    fn num(&mut self, n: i64) -> Self::Node { self.bump.alloc(BumpExpr::Num(n)) }
    fn var(&mut self, s: u8) -> Self::Node { self.bump.alloc(BumpExpr::Var(s)) }
    fn bin(&mut self, op: u8, a: Self::Node, b: Self::Node) -> Self::Node { self.bump.alloc(BumpExpr::Bin(op, a, b)) }
}
fn eval_bump(e: &BumpExpr, env: &[i64]) -> i64 {
    match e {
        BumpExpr::Num(n) => *n,
        BumpExpr::Var(s) => env[*s as usize],
        BumpExpr::Bin(op, a, b) => apply(*op, eval_bump(a, env), eval_bump(b, env)),
    }
}

fn best<R>(f: &mut impl FnMut() -> R) -> f64 {
    (0..5).map(|_| { let t = Instant::now(); std::hint::black_box(f()); t.elapsed().as_secs_f64() }).fold(f64::MAX, f64::min)
}

fn main() {
    // 100,000 small expressions: (a + 12) * (b - 7) / 3 + c * 2 ... with varying constants.
    let exprs: Vec<String> = (0..100_000)
        .map(|i| format!("(a + {}) * (b - {}) / {} + c * {} - {}", i % 97, i % 13, 1 + i % 5, i % 7, i % 11))
        .collect();
    let env = [5i64, 9, 2];
    println!("node sizes: BoxExpr {} B (a Box adds no header), arena Node {} B, BumpExpr {} B",
        size_of::<BoxExpr>(), size_of::<Node>(), size_of::<BumpExpr>());

    // Box
    let (a0, f0) = counting::snapshot();
    let trees: Vec<Box<BoxExpr>> = exprs.iter().map(|s| P { s: s.as_bytes(), i: 0 }.expr(&mut BoxB)).collect();
    let (a1, _) = counting::snapshot();
    let sum_box: i64 = trees.iter().map(|t| eval_box(t, &env)).sum();
    drop(trees);
    let (_, f1) = counting::snapshot();
    println!("Box:   allocs {:>9}  frees {:>9}  sum {sum_box}", a1 - a0, f1 - f0);

    // arena
    let (a0, f0) = counting::snapshot();
    let mut arena = Arena { nodes: Vec::new() };
    let roots: Vec<u32> = exprs.iter().map(|s| P { s: s.as_bytes(), i: 0 }.expr(&mut arena)).collect();
    let (a1, _) = counting::snapshot();
    let sum_arena: i64 = roots.iter().map(|&r| eval_arena(&arena.nodes, r, &env)).sum();
    let nodes = arena.nodes.len();
    drop((arena, roots));
    let (_, f1) = counting::snapshot();
    println!("arena: allocs {:>9}  frees {:>9}  sum {sum_arena}  ({nodes} nodes)", a1 - a0, f1 - f0);

    // bumpalo
    let (a0, f0) = counting::snapshot();
    let bump = bumpalo::Bump::new();
    let broots: Vec<&BumpExpr> = exprs.iter().map(|s| P { s: s.as_bytes(), i: 0 }.expr(&mut BumpB { bump: &bump })).collect();
    let (a1, _) = counting::snapshot();
    let sum_bump: i64 = broots.iter().map(|r| eval_bump(r, &env)).sum();
    let bytes = bump.allocated_bytes();
    drop(broots);
    drop(bump);
    let (_, f1) = counting::snapshot();
    println!("bump:  allocs {:>9}  frees {:>9}  sum {sum_bump}  ({bytes} bytes in chunks)", a1 - a0, f1 - f0);
    assert!(sum_box == sum_arena && sum_arena == sum_bump);

    // Timing: parse + evaluate + drop, best of 5.
    let t_box = best(&mut || {
        let v: Vec<Box<BoxExpr>> = exprs.iter().map(|s| P { s: s.as_bytes(), i: 0 }.expr(&mut BoxB)).collect();
        v.iter().map(|t| eval_box(t, &env)).sum::<i64>()
    });
    let t_arena = best(&mut || {
        let mut a = Arena { nodes: Vec::new() };
        let r: Vec<u32> = exprs.iter().map(|s| P { s: s.as_bytes(), i: 0 }.expr(&mut a)).collect();
        r.iter().map(|&r| eval_arena(&a.nodes, r, &env)).sum::<i64>()
    });
    let t_bump = best(&mut || {
        let bump = bumpalo::Bump::new();
        let r: Vec<&BumpExpr> = exprs.iter().map(|s| P { s: s.as_bytes(), i: 0 }.expr(&mut BumpB { bump: &bump })).collect();
        r.iter().map(|r| eval_bump(r, &env)).sum::<i64>()
    });
    println!("parse+eval+drop (release, best of 5, one run): Box {:.1} ms, arena {:.1} ms, bump {:.1} ms",
        t_box * 1e3, t_arena * 1e3, t_bump * 1e3);
}
