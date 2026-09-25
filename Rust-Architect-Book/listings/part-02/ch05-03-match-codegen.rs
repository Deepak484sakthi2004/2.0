// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target asm -Mode release
#[inline(never)]
pub fn weight(class: u8) -> u32 {
    match class {
        0 => 10,
        1 => 25,
        2 => 40,
        3 => 55,
        4 => 90,
        5 => 120,
        6 => 200,
        7 => 350,
        _ => 0,
    }
}

#[inline(never)]
pub fn sparse(code: u16) -> u32 {
    match code {
        200 => 1,
        404 => 2,
        503 => 3,
        _ => 0,
    }
}

pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Neg,
    Abs,
}

#[inline(never)]
pub fn apply(op: Op, a: i64, b: i64) -> i64 {
    match op {
        Op::Add => a.wrapping_add(b),
        Op::Sub => a.wrapping_sub(b),
        Op::Mul => a.wrapping_mul(b),
        Op::Div => if b == 0 { 0 } else { a.wrapping_div(b) },
        Op::Neg => a.wrapping_neg(),
        Op::Abs => a.wrapping_abs(),
    }
}
