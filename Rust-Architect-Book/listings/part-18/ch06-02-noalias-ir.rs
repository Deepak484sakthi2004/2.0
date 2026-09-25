// verify: release build
// Source for the LLVM IR / asm artifacts in Chapter 18.6: the attributes from ch06-01, as LLVM sees
// them, and the optimization they permit (one load of *src instead of two).
#[inline(never)]
pub fn add_into(dst: &mut i32, src: &i32) {
    *dst += *src;
    *dst += *src; // with `noalias`, *src need not be reloaded after the first store
}
