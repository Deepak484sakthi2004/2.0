// verify: debug build
// verify: release build
// Listing 17.1-2: one tiny function, to be viewed at every level rustc produces
// (HIR, MIR, LLVM IR, assembly) with tools/emit.ps1.

#[inline(never)]
pub fn scale(x: i64) -> i64 {
    let y = x * 3;
    y + 1
}
