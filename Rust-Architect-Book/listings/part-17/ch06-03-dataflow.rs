// verify: debug ok
// Listing 17.6-3: two classic dataflow analyses on a CFG, solved by iterating to a fixpoint.
//   definite assignment: forward, "must" (meet = intersection) -- Java's JLS ch. 16, Rust's E0381
//   liveness:            backward, "may" (meet = union)        -- register allocation, pruned SSA
// Variables are bits in a u64; each block is a list of Def/Use events plus its successors.

#[derive(Clone, Copy)]
enum Ev { Def(u8), Use(u8) }

struct Cfg { names: Vec<&'static str>, blocks: Vec<(Vec<Ev>, Vec<usize>)> }

fn set_str(cfg: &Cfg, s: u64) -> String {
    let v: Vec<&str> = (0..cfg.names.len()).filter(|&i| s & (1 << i) != 0).map(|i| cfg.names[i]).collect();
    format!("{{{}}}", v.join(","))
}

fn preds(cfg: &Cfg) -> Vec<Vec<usize>> {
    let mut p = vec![Vec::new(); cfg.blocks.len()];
    for (b, (_, succs)) in cfg.blocks.iter().enumerate() {
        for &s in succs { p[s].push(b); }
    }
    p
}

/// Forward must-analysis. IN[entry] = params; IN[b] = AND of OUT[preds]; OUT = IN | defs.
fn definite_assignment(cfg: &Cfg, params: u64) -> (Vec<u64>, usize) {
    let n = cfg.blocks.len();
    let preds = preds(cfg);
    let all = (1u64 << cfg.names.len()) - 1;
    let mut out = vec![all; n]; // optimistic start ("top") for a must-analysis
    let mut rounds = 0;
    loop {
        rounds += 1;
        let mut changed = false;
        for b in 0..n {
            let inn = if b == 0 { params } else { preds[b].iter().fold(all, |acc, &p| acc & out[p]) };
            let defs = cfg.blocks[b].0.iter().fold(0, |acc, e| match e { Ev::Def(v) => acc | 1 << v, _ => acc });
            let new = inn | defs;
            if new != out[b] { out[b] = new; changed = true; }
        }
        if !changed { break; }
    }
    // Report uses that are not definitely assigned on every path.
    let mut ins = vec![0u64; n];
    for b in 0..n {
        let mut cur = if b == 0 { params } else { preds[b].iter().fold(all, |acc, &p| acc & out[p]) };
        ins[b] = cur;
        for e in &cfg.blocks[b].0 {
            match *e {
                Ev::Def(v) => cur |= 1 << v,
                Ev::Use(v) if cur & (1 << v) == 0 => {
                    println!("  error: `{}` is possibly unassigned when used in bb{b}", cfg.names[v as usize]);
                }
                Ev::Use(_) => {}
            }
        }
    }
    (ins, rounds)
}

/// Backward may-analysis. OUT[b] = OR of IN[succs]; IN = uses-before-defs | (OUT - defs).
fn liveness(cfg: &Cfg) -> (Vec<u64>, usize) {
    let n = cfg.blocks.len();
    let mut live_in = vec![0u64; n]; // pessimistic start ("bottom") for a may-analysis
    let mut rounds = 0;
    loop {
        rounds += 1;
        let mut changed = false;
        for b in (0..n).rev() {
            let mut live = cfg.blocks[b].1.iter().fold(0, |acc, &s| acc | live_in[s]);
            for e in cfg.blocks[b].0.iter().rev() {
                match *e {
                    Ev::Def(v) => live &= !(1 << v),
                    Ev::Use(v) => live |= 1 << v,
                }
            }
            if live != live_in[b] { live_in[b] = live; changed = true; }
        }
        if !changed { break; }
    }
    (live_in, rounds)
}

fn main() {
    use Ev::*;
    // (a) let x; if c { x = 1 } ; y = x + 1
    let diamond = Cfg {
        names: vec!["c", "x", "y"],
        blocks: vec![
            (vec![Use(0)], vec![1, 2]),       // bb0: branch on c
            (vec![Def(1)], vec![3]),          // bb1: x = 1
            (vec![], vec![3]),                // bb2: (x not assigned)
            (vec![Use(1), Def(2)], vec![]),   // bb3: y = x + 1
        ],
    };
    println!("(a) definite assignment on the diamond:");
    let (ins, rounds) = definite_assignment(&diamond, 1 << 0);
    for (b, s) in ins.iter().enumerate() { println!("  IN(bb{b}) = {}", set_str(&diamond, *s)); }
    println!("  fixpoint after {rounds} rounds");

    // (b) sum_to from listing 17.6-1: bb0 init, bb1 loop test, bb2 body, bb3 exit
    let sum_to = Cfg {
        names: vec!["n", "i", "total", "%0", "%1", "%2"],
        blocks: vec![
            (vec![Def(1), Def(2)], vec![1]),                                           // i = 0; total = 0
            (vec![Use(1), Use(0), Def(3), Use(3)], vec![2, 3]),                       // %0 = i < n; branch %0
            (vec![Use(1), Def(4), Use(4), Def(1), Use(2), Use(1), Def(5), Use(5), Def(2)], vec![1]), // i = i+1; total = total+i
            (vec![Use(2)], vec![]),                                                    // return total
        ],
    };
    println!("\n(b) sum_to: definite assignment finds nothing wrong:");
    let (_, rounds) = definite_assignment(&sum_to, 1 << 0);
    println!("  (no errors) fixpoint after {rounds} rounds");
    println!("(b) liveness:");
    let (live_in, rounds) = liveness(&sum_to);
    for (b, s) in live_in.iter().enumerate() { println!("  live-in(bb{b}) = {}", set_str(&sum_to, *s)); }
    println!("  fixpoint after {rounds} rounds (the back edge bb2 -> bb1 needs a second pass)");
    println!("  pruned SSA: bb1 needs phis only for its live-in, multiply-defined variables: i, total");
}
