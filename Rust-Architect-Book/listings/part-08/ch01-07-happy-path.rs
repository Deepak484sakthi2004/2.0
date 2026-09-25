// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target asm -Mode release
use std::num::ParseIntError;

#[inline(never)]
pub fn parse_u32(s: &str) -> Result<u32, ParseIntError> {
    s.parse()
}

#[inline(never)]
pub fn sum_two(a: &str, b: &str) -> Result<u32, ParseIntError> {
    let x = parse_u32(a)?;
    let y = parse_u32(b)?;
    Ok(x.wrapping_add(y))
}
