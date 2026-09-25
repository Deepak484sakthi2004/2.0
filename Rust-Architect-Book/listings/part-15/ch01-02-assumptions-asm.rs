// verify: release build
// What the optimizer assumes from validity invariants (inspect with tools/emit.ps1 -Target asm).
#[inline(never)]
pub fn bool_to_u32(b: bool) -> u32 {
    if b { 1 } else { 0 } // bool is 0 or 1, so this is just a zero-extension
}

#[inline(never)]
pub fn is_null_ref(r: &u64) -> bool {
    (r as *const u64).is_null() // a reference is never null: folds to `false`
}

#[inline(never)]
pub fn is_null_raw(p: *const u64) -> bool {
    p.is_null() // a raw pointer may be null: a real test
}
