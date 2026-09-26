// verify: debug ok
// Listing 17.4-4 (debugging exercise): two resolvers for `let` statements. They differ in ONE
// ordering decision, and one of them makes this program compute the wrong answer.

#[derive(Debug)]
enum E {
    Num(i64),
    Var(&'static str),
    Add(Box<E>, Box<E>),
}

struct Let {
    name: &'static str,
    init: E,
}

/// Resolved form: variables become slot numbers.
#[derive(Debug)]
enum R {
    Num(i64),
    Slot(usize),
    Add(Box<R>, Box<R>),
}

fn resolve_expr(e: &E, scope: &[(&str, usize)]) -> R {
    match e {
        E::Num(n) => R::Num(*n),
        // innermost (latest) binding wins: search from the end
        E::Var(v) => R::Slot(scope.iter().rev().find(|(n, _)| n == v).expect("bound").1),
        E::Add(a, b) => R::Add(Box::new(resolve_expr(a, scope)), Box::new(resolve_expr(b, scope))),
    }
}

fn resolve(prog: &[Let], declare_before_init: bool) -> Vec<(usize, R)> {
    let mut scope: Vec<(&str, usize)> = Vec::new();
    let mut out = Vec::new();
    for (slot, l) in prog.iter().enumerate() {
        if declare_before_init {
            scope.push((l.name, slot));
            out.push((slot, resolve_expr(&l.init, &scope)));
        } else {
            out.push((slot, resolve_expr(&l.init, &scope)));
            scope.push((l.name, slot));
        }
    }
    out
}

fn eval(r: &R, slots: &[i64]) -> i64 {
    match r {
        R::Num(n) => *n,
        R::Slot(s) => slots[*s],
        R::Add(a, b) => eval(a, slots) + eval(b, slots),
    }
}

fn main() {
    use E::*;
    // let x = 1;  let x = x + 1;  let y = x + x;   (y should be 4)
    let prog = [
        Let { name: "x", init: Num(1) },
        Let { name: "x", init: Add(Box::new(Var("x")), Box::new(Num(1))) },
        Let { name: "y", init: Add(Box::new(Var("x")), Box::new(Var("x"))) },
    ];
    for mode in [false, true] {
        let resolved = resolve(&prog, mode);
        let mut slots = vec![0i64; prog.len()]; // slots start zeroed, like fresh stack memory in a VM
        for (slot, r) in &resolved {
            slots[*slot] = eval(r, &slots);
        }
        println!("declare_before_init = {mode}:");
        for (slot, r) in &resolved {
            println!("  slot {slot} ({}) = {r:?}", prog[*slot].name);
        }
        println!("  y = {}", slots[2]);
    }
}
