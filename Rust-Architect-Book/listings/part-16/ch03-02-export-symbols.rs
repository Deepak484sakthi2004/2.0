// verify: release build
// For tools/emit.ps1: what `#[unsafe(no_mangle)] extern "C"` changes in the output, next to an
// ordinary Rust function (v0-mangled) and an `extern "C"` function that may panic.
#[unsafe(no_mangle)]
pub extern "C" fn meridian_abi_version() -> u32 {
    3
}

#[inline(never)]
pub fn abi_version_rust() -> u32 {
    3
}

/// May panic (division by zero is checked): the C ABI can't unwind, so rustc adds an abort path.
#[unsafe(no_mangle)]
pub extern "C" fn meridian_ratio(a: u32, b: u32) -> u32 {
    a / b
}
