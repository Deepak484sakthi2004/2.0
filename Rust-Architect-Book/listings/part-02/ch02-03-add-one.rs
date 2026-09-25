// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target asm -Mode debug   (and -Mode release)
#[inline(never)]
pub fn add_one(x: u32) -> u32 {
    x + 1
}
