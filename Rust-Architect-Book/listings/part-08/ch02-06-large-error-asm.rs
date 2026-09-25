// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target asm -Mode release
pub struct BigError {
    pub context: [u8; 512],
    pub code: u32,
}

#[inline(never)]
pub fn check_big(x: u64) -> Result<u64, BigError> {
    if x < 1_000 { Ok(x * 2) } else { Err(BigError { context: [0xEE; 512], code: 7 }) }
}

#[inline(never)]
pub fn outer_big(x: u64) -> Result<u64, BigError> {
    let v = check_big(x)?;
    Ok(v + 1)
}

#[inline(never)]
pub fn check_boxed(x: u64) -> Result<u64, Box<BigError>> {
    if x < 1_000 { Ok(x * 2) } else { Err(Box::new(BigError { context: [0xEE; 512], code: 7 })) }
}

#[inline(never)]
pub fn outer_boxed(x: u64) -> Result<u64, Box<BigError>> {
    let v = check_boxed(x)?;
    Ok(v + 1)
}
