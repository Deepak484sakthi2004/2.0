// verify: debug build
// verify: release build
// Listing 17.6-5: the same loop as Ore's sum_to, compiled by rustc. Emit its MIR (a CFG of basic
// blocks over mutable locals: NOT SSA) and its LLVM IR (debug: allocas and loads/stores; release:
// SSA values joined by phi). `black_box` keeps LLVM from replacing the loop with a formula.

#[inline(never)]
pub fn sum_to(n: i64) -> i64 {
    let mut i = 0;
    let mut total = 0;
    while i < n {
        i += 1;
        total += std::hint::black_box(i);
    }
    total
}
