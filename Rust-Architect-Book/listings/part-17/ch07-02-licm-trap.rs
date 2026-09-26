// verify: debug ok
// Listing 17.7-2: loop-invariant code motion (LICM) and the one rule it must not break.
// `b / a` is loop-invariant, but it only runs when `a != 0` and only if the loop body runs at all.
// Hoisting it unconditionally into the preheader makes a correct program fail.
//
//   source:  acc = 0; i = 0; while i < n { if a != 0 { acc = acc + b / a }  i = i + 1 }  return acc

#[derive(Clone, Debug)]
enum I {
    Set(&'static str, i64),                                // x = constant
    Op(&'static str, &'static str, char, &'static str),    // x = y op z
    Br(&'static str, usize, usize),                        // if x goto t else e
    Jmp(usize),
    Ret(&'static str),
}

fn program() -> Vec<Vec<I>> {
    use I::*;
    vec![
        vec![Set("acc", 0), Set("i", 0), Set("zero", 0), Set("one", 1), Jmp(1)], // bb0: preheader
        vec![Op("c", "i", '<', "n"), Br("c", 2, 5)],                              // bb1: loop header
        vec![Op("nz", "a", '!', "zero"), Br("nz", 3, 4)],                         // bb2: if a != 0
        vec![Op("q", "b", '/', "a"), Op("acc", "acc", '+', "q"), Jmp(4)],         // bb3: acc += b / a
        vec![Op("i", "i", '+', "one"), Jmp(1)],                                   // bb4: i += 1
        vec![Ret("acc")],                                                         // bb5: exit
    ]
}

const LOOP: [usize; 4] = [1, 2, 3, 4];

fn defined_in_loop(p: &[Vec<I>], var: &str) -> bool {
    LOOP.iter().any(|&b| p[b].iter().any(|i| matches!(i, I::Op(d, ..) | I::Set(d, _) if *d == var)))
}

fn may_trap(i: &I) -> bool {
    matches!(i, I::Op(_, _, '/', _))
}


fn licm(p: &mut Vec<Vec<I>>, safe: bool) -> Vec<String> {
    let mut moved = Vec::new();
    for &b in &LOOP {
        let mut k = 0;
        while k < p[b].len() {
            let hoist = match &p[b][k] {
                I::Op(_, x, _, y) => {
                    let invariant = !defined_in_loop(p, x) && !defined_in_loop(p, y);
                    // Legal to hoist: the instruction cannot trap, OR it would have executed anyway (its block runs
                    // on every iteration AND the loop provably runs at least once). This pass checks only the first.
                    invariant && (!safe || !may_trap(&p[b][k]))
                }
                _ => false,
            };
            if hoist {
                let inst = p[b].remove(k);
                moved.push(format!("{inst:?} from bb{b}"));
                let at = p[0].len() - 1; // before the preheader's jump
                p[0].insert(at, inst);
            } else {
                k += 1;
            }
        }
    }
    moved
}

fn run(p: &[Vec<I>], n: i64, a: i64, b: i64) -> Result<(i64, u64), String> {
    let mut env = std::collections::HashMap::from([("n", n), ("a", a), ("b", b)]);
    let (mut bb, mut divs) = (0, 0u64);
    loop {
        for inst in &p[bb] {
            match inst {
                I::Set(d, c) => { env.insert(*d, *c); }
                I::Op(d, x, op, y) => {
                    let (x, y) = (env[x], env[y]);
                    let v = match op {
                        '<' => (x < y) as i64,
                        '!' => (x != y) as i64,
                        '+' => x + y,
                        '/' => {
                            divs += 1;
                            if y == 0 { return Err("division by zero".into()); }
                            x / y
                        }
                        _ => unreachable!(),
                    };
                    env.insert(*d, v);
                }
                I::Br(c, t, e) => { bb = if env[c] != 0 { *t } else { *e }; break; }
                I::Jmp(t) => { bb = *t; break; }
                I::Ret(x) => return Ok((env[x], divs)),
            }
        }
    }
}

fn main() {
    let original = program();
    let mut naive = program();
    let mut safe = program();
    println!("naive LICM hoists: {:?}", licm(&mut naive, false));
    println!("safe  LICM hoists: {:?}", licm(&mut safe, true));
    println!();
    println!("{:<18} {:<26} {:<26} {:<26}", "(n, a, b)", "original", "naive LICM", "safe LICM");
    for (n, a, b) in [(1000, 2, 10), (1000, 0, 10), (0, 0, 10), (0, 5, 10)] {
        let show = |r: Result<(i64, u64), String>| match r {
            Ok((v, d)) => format!("{v} ({d} divisions)"),
            Err(e) => format!("ERROR: {e}"),
        };
        println!("{:<18} {:<26} {:<26} {:<26}", format!("({n}, {a}, {b})"),
            show(run(&original, n, a, b)), show(run(&naive, n, a, b)), show(run(&safe, n, a, b)));
    }
}
