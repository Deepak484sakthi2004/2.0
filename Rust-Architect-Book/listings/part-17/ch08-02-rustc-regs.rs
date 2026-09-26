// verify: release build
// Listing 17.8-2: what LLVM's register allocator and the System V x86-64 calling convention do with
// real functions. Emit release assembly and look for: arguments in rdi, rsi, rdx, rcx, r8, r9 (the 7th
// on the stack); values kept alive across a call moved into callee-saved registers (push/pop rbx...).

#[inline(never)]
pub fn seven(a: i64, b: i64, c: i64, d: i64, e: i64, f: i64, g: i64) -> i64 {
    a + 2 * b + 3 * c + 4 * d + 5 * e + 6 * f + 7 * g
}

#[inline(never)]
pub fn opaque(x: i64) -> i64 {
    std::hint::black_box(x)
}

/// `p`, `q`, `x`, `y` are needed after the call: they must survive it.
#[inline(never)]
pub fn across_call(x: i64, y: i64) -> i64 {
    let p = x * 3;
    let q = y * 5;
    let r = opaque(p + q);
    r + p + q + x + y
}
