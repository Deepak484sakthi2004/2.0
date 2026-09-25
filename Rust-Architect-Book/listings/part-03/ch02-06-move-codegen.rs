// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target mir -Mode debug   and   -Target asm -Mode release
pub struct Wrapper {
    pub s: String,
    pub n: u64,
}

#[inline(never)]
pub fn pass_along(n: u64, s: String) -> (u64, String) {
    (n, s) // MIR spells this out: `copy` for the u64, `move` for the String
}

#[inline(never)]
pub fn wrap(s: String) -> Wrapper {
    Wrapper { s, n: 1 }
}
