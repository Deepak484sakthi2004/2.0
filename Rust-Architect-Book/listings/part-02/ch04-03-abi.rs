// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target asm -Mode release
#[inline(never)]
pub fn split(x: u64) -> (u64, u64) {
    (x >> 32, x & 0xFFFF_FFFF)
}

#[inline(never)]
pub fn table(seed: u64) -> [u64; 8] {
    let mut t = [0u64; 8];
    for i in 0..8 {
        t[i] = seed.wrapping_mul(i as u64 + 1);
    }
    t
}
