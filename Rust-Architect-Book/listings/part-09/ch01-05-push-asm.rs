// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target asm -Mode release
#[inline(never)]
pub fn push_one(v: &mut Vec<u64>, x: u64) {
    v.push(x);
}
