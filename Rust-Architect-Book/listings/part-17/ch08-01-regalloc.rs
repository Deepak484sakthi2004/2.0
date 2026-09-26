// verify: debug ok
// Listing 17.8-1: the back end, end to end. Three-address code with unlimited virtual registers ->
// liveness -> live intervals -> linear-scan register allocation (Poletto & Sarkar 1999) with spilling
// -> x86-64-flavored two-address assembly text. The Playground can't assemble or run that text, so a
// small emulator executes it and every result is compared with an interpreter of the input code.

use std::collections::HashMap;

// ---------------- the input: three-address code over virtual registers ----------------
#[derive(Debug, Clone, Copy, PartialEq)]
enum Opnd { V(u32), Imm(i64), Arg(u32) }

#[derive(Debug, Clone)]
enum Tac {
    Bin(u32, char, Opnd, Opnd), // v = a op b   (op: + - *)
    Copy(u32, Opnd),
    Label(u32),
    JumpIfGe(Opnd, Opnd, u32),
    Jump(u32),
    Ret(Opnd),
}

/// f(n, x, y): seven values computed up front stay live across a loop that uses all of them.
fn program() -> Vec<Tac> {
    use Opnd::*;
    use Tac::*;
    let (n, x, y) = (Arg(0), Arg(1), Arg(2));
    vec![
        Bin(1, '+', x, Imm(1)),   // a = x + 1
        Bin(2, '+', y, Imm(2)),   // b = y + 2
        Bin(3, '*', x, y),        // c = x * y
        Bin(4, '+', V(1), V(2)),  // d = a + b
        Bin(5, '-', V(3), V(1)),  // e = c - a
        Bin(6, '*', V(4), V(5)),  // f = d * e
        Bin(7, '+', V(2), V(3)),  // g = b + c
        Copy(8, Imm(0)),          // acc = 0
        Copy(9, Imm(0)),          // i = 0
        Label(0),                 // loop:
        JumpIfGe(V(9), n, 1),     //   if i >= n goto done
        Bin(10, '*', V(1), V(9)), //   t1 = a * i
        Bin(11, '+', V(10), V(2)),//   t2 = t1 + b
        Bin(12, '+', V(11), V(3)),//   t3 = t2 + c
        Bin(13, '-', V(12), V(4)),//   t4 = t3 - d
        Bin(14, '+', V(13), V(5)),//   t5 = t4 + e
        Bin(15, '-', V(14), V(6)),//   t6 = t5 - f
        Bin(16, '+', V(15), V(7)),//   t7 = t6 + g
        Bin(8, '+', V(8), V(16)), //   acc = acc + t7
        Bin(9, '+', V(9), Imm(1)),//   i = i + 1
        Jump(0),
        Label(1),                 // done:
        Ret(V(8)),
    ]
}

fn apply(op: char, a: i64, b: i64) -> i64 {
    match op { '+' => a.wrapping_add(b), '-' => a.wrapping_sub(b), _ => a.wrapping_mul(b) }
}

fn interpret(code: &[Tac], args: &[i64]) -> i64 {
    let mut v: HashMap<u32, i64> = HashMap::new();
    let val = |v: &HashMap<u32, i64>, o: Opnd| match o { Opnd::V(r) => v[&r], Opnd::Imm(c) => c, Opnd::Arg(i) => args[i as usize] };
    let label = |l: u32| code.iter().position(|t| matches!(t, Tac::Label(x) if *x == l)).unwrap();
    let mut pc = 0;
    loop {
        match &code[pc] {
            Tac::Bin(d, op, a, b) => { let r = apply(*op, val(&v, *a), val(&v, *b)); v.insert(*d, r); }
            Tac::Copy(d, a) => { let r = val(&v, *a); v.insert(*d, r); }
            Tac::Label(_) => {}
            Tac::JumpIfGe(a, b, l) => if val(&v, *a) >= val(&v, *b) { pc = label(*l); continue; },
            Tac::Jump(l) => { pc = label(*l); continue; }
            Tac::Ret(a) => return val(&v, *a),
        }
        pc += 1;
    }
}

// ---------------- liveness and live intervals ----------------
fn uses_defs(t: &Tac) -> (Vec<u32>, Option<u32>) {
    let vs = |os: &[Opnd]| os.iter().filter_map(|o| if let Opnd::V(r) = o { Some(*r) } else { None }).collect();
    match t {
        Tac::Bin(d, _, a, b) => (vs(&[*a, *b]), Some(*d)),
        Tac::Copy(d, a) => (vs(&[*a]), Some(*d)),
        Tac::JumpIfGe(a, b, _) => (vs(&[*a, *b]), None),
        Tac::Ret(a) => (vs(&[*a]), None),
        Tac::Label(_) | Tac::Jump(_) => (vec![], None),
    }
}

/// Backward dataflow over the instruction list; returns live-out per instruction.
fn liveness(code: &[Tac]) -> Vec<Vec<u32>> {
    let label = |l: u32| code.iter().position(|t| matches!(t, Tac::Label(x) if *x == l)).unwrap();
    let succ = |i: usize| -> Vec<usize> {
        match &code[i] {
            Tac::Jump(l) => vec![label(*l)],
            Tac::JumpIfGe(_, _, l) => vec![i + 1, label(*l)],
            Tac::Ret(_) => vec![],
            _ => vec![i + 1],
        }
    };
    let mut live_in: Vec<Vec<u32>> = vec![vec![]; code.len()];
    let mut live_out: Vec<Vec<u32>> = vec![vec![]; code.len()];
    loop {
        let mut changed = false;
        for i in (0..code.len()).rev() {
            let mut out: Vec<u32> = succ(i).iter().flat_map(|&s| live_in[s].clone()).collect();
            out.sort();
            out.dedup();
            let (uses, def) = uses_defs(&code[i]);
            let mut inn: Vec<u32> = out.iter().copied().filter(|v| Some(*v) != def).chain(uses).collect();
            inn.sort();
            inn.dedup();
            if inn != live_in[i] || out != live_out[i] { live_in[i] = inn; live_out[i] = out; changed = true; }
        }
        if !changed { return live_out; }
    }
}

/// One interval per virtual register: [first point it is defined or live, last point it is used or live].
fn intervals(code: &[Tac], live_out: &[Vec<u32>]) -> Vec<(u32, usize, usize)> {
    let mut span: HashMap<u32, (usize, usize)> = HashMap::new();
    let mut touch = |v: u32, i: usize| {
        let e = span.entry(v).or_insert((i, i));
        e.0 = e.0.min(i);
        e.1 = e.1.max(i);
    };
    for (i, t) in code.iter().enumerate() {
        let (uses, def) = uses_defs(t);
        uses.into_iter().chain(def).chain(live_out[i].iter().copied()).for_each(|v| touch(v, i));
    }
    let mut out: Vec<(u32, usize, usize)> = span.into_iter().map(|(v, (s, e))| (v, s, e)).collect();
    out.sort_by_key(|&(v, s, _)| (s, v));
    out
}

// ---------------- linear scan ----------------
#[derive(Debug, Clone, Copy, PartialEq)]
enum Loc { Reg(usize), Slot(usize) }

const REGS: [&str; 12] = ["rcx", "rdx", "rsi", "rdi", "r8", "r9", "r10", "rbx", "r12", "r13", "r14", "r15"];
const SCRATCH: &str = "r11"; // reserved for memory-to-memory moves; rax holds the return value

fn linear_scan(ivs: &[(u32, usize, usize)], k: usize) -> HashMap<u32, Loc> {
    let mut loc = HashMap::new();
    let mut active: Vec<(usize, u32, usize)> = Vec::new(); // (end, vreg, reg), kept sorted by end
    let mut free: Vec<usize> = (0..k).rev().collect();
    let mut slots = 0;
    for &(v, start, end) in ivs {
        // expire intervals that ended before this one starts
        active.retain(|&(e, _, r)| if e < start { free.push(r); false } else { true });
        if let Some(r) = free.pop() {
            loc.insert(v, Loc::Reg(r));
            active.push((end, v, r));
        } else {
            // spill heuristic: evict whichever interval ends LAST (it would block a register longest)
            let &(last_end, last_v, last_r) = active.last().unwrap();
            if last_end > end {
                loc.insert(last_v, Loc::Slot(slots));
                loc.insert(v, Loc::Reg(last_r));
                active.pop();
                active.push((end, v, last_r));
            } else {
                loc.insert(v, Loc::Slot(slots));
            }
            slots += 1;
        }
        active.sort();
    }
    loc
}

// ---------------- instruction selection + rewriting with the allocation ----------------
fn place(o: Opnd, loc: &HashMap<u32, Loc>) -> String {
    match o {
        Opnd::V(v) => match loc[&v] { Loc::Reg(r) => REGS[r].to_string(), Loc::Slot(s) => format!("[rsp+{}]", 8 * s) },
        Opnd::Imm(c) => c.to_string(),
        Opnd::Arg(i) => format!("[args+{}]", 8 * i),
    }
}

fn is_mem(s: &str) -> bool { s.starts_with('[') }

fn select(code: &[Tac], loc: &HashMap<u32, Loc>) -> Vec<String> {
    let mut out = Vec::new();
    for t in code {
        match t {
            Tac::Copy(d, a) => {
                let (d, a) = (place(Opnd::V(*d), loc), place(*a, loc));
                if is_mem(&d) && is_mem(&a) {
                    out.push(format!("mov {SCRATCH}, {a}"));
                    out.push(format!("mov {d}, {SCRATCH}"));
                } else {
                    out.push(format!("mov {d}, {a}"));
                }
            }
            Tac::Bin(d, op, a, b) => {
                let mnem = match op { '+' => "add", '-' => "sub", _ => "imul" };
                let (d, a, b) = (place(Opnd::V(*d), loc), place(*a, loc), place(*b, loc));
                // x86 is two-address: dst = dst op src. Work in the scratch register when the
                // destination is in memory (imul can't write memory), or when dst would clobber b.
                if is_mem(&d) || d == b {
                    out.push(format!("mov {SCRATCH}, {a}"));
                    out.push(format!("{mnem} {SCRATCH}, {b}"));
                    out.push(format!("mov {d}, {SCRATCH}"));
                } else {
                    if d != a { out.push(format!("mov {d}, {a}")); }
                    out.push(format!("{mnem} {d}, {b}"));
                }
            }
            Tac::Label(l) => out.push(format!(".L{l}:")),
            Tac::JumpIfGe(a, b, l) => {
                let (a, b) = (place(*a, loc), place(*b, loc));
                if is_mem(&a) || a.parse::<i64>().is_ok() {
                    out.push(format!("mov {SCRATCH}, {a}"));
                    out.push(format!("cmp {SCRATCH}, {b}"));
                } else {
                    out.push(format!("cmp {a}, {b}"));
                }
                out.push(format!("jge .L{l}"));
            }
            Tac::Jump(l) => out.push(format!("jmp .L{l}")),
            Tac::Ret(a) => {
                out.push(format!("mov rax, {}", place(*a, loc)));
                out.push("ret".to_string());
            }
        }
    }
    out
}

// ---------------- an emulator for the generated text ----------------
fn read(m: &HashMap<String, i64>, o: &str) -> i64 {
    o.parse::<i64>().unwrap_or_else(|_| m[o])
}

fn write(m: &mut HashMap<String, i64>, o: &str, v: i64) {
    m.insert(o.to_string(), v);
}

fn emulate(asm: &[String], args: &[i64]) -> (i64, u64) {
    let mut m: HashMap<String, i64> = HashMap::new();
    for (i, a) in args.iter().enumerate() { write(&mut m, &format!("[args+{}]", 8 * i), *a); }
    let (mut pc, mut flag_ge, mut mem_ops) = (0, false, 0u64);
    loop {
        let line = &asm[pc];
        mem_ops += line.matches('[').count() as u64;
        let (mnem, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        let ops: Vec<&str> = rest.split(", ").collect();
        match mnem {
            "mov" => { let v = read(&m, ops[1]); write(&mut m, ops[0], v); }
            "add" | "sub" | "imul" => {
                let op = match mnem { "add" => '+', "sub" => '-', _ => '*' };
                let v = apply(op, read(&m, ops[0]), read(&m, ops[1]));
                write(&mut m, ops[0], v);
            }
            "cmp" => flag_ge = read(&m, ops[0]) >= read(&m, ops[1]),
            "jge" | "jmp" if mnem == "jmp" || flag_ge => {
                pc = asm.iter().position(|l| *l == format!("{}:", ops[0])).unwrap();
                continue;
            }
            "jge" => {}
            "ret" => return (m["rax"], mem_ops),
            _ if mnem.ends_with(':') => {}
            other => panic!("unknown instruction {other}"),
        }
        pc += 1;
    }
}

fn main() {
    let code = program();
    let live_out = liveness(&code);
    let ivs = intervals(&code, &live_out);
    let pressure = live_out.iter().map(|s| s.len()).max().unwrap();
    println!("{} virtual registers; register pressure (max simultaneously live) = {pressure}", ivs.len());
    let shown: Vec<String> = ivs.iter().map(|(v, s, e)| format!("v{v}:[{s},{e}]")).collect();
    println!("live intervals: {}", shown.join(" "));

    let inputs = [[5i64, 3, 4], [0, 1, 1], [10, -7, 2], [3, 100, -50]];
    let expected: Vec<i64> = inputs.iter().map(|a| interpret(&code, a)).collect();
    println!("\n  K  spilled  instructions  memory operands executed (for n=10)");
    for k in [2, 3, 4, 6, 8, 12] {
        let loc = linear_scan(&ivs, k);
        let spilled = loc.values().filter(|l| matches!(l, Loc::Slot(_))).count();
        let asm = select(&code, &loc);
        for (a, want) in inputs.iter().zip(&expected) {
            assert_eq!(emulate(&asm, a).0, *want, "K={k} args={a:?}");
        }
        let (_, mem) = emulate(&asm, &inputs[2]);
        println!("{k:>3}  {spilled:>7}  {:>12}  {mem:>8}", asm.len());
        if k == 4 {
            let listing = asm.clone();
            println!("\n  --- generated code for K = 4 ---");
            for l in &listing { println!("    {}{l}", if l.ends_with(':') { "" } else { "    " }); }
            println!();
        }
    }
    println!("all allocations produce the interpreter's results: {expected:?}");
}
