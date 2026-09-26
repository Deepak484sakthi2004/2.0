// verify: debug error:improper_ctypes_definitions
// The niche guarantee covers pointer-like types only. Option<u32> has no niche and no defined
// C layout, so it can't appear in a C signature.
#![deny(improper_ctypes_definitions)]

#[unsafe(no_mangle)]
pub extern "C" fn meridian_block_threshold(override_score: Option<u32>) -> u32 {
    override_score.unwrap_or(80)
}

fn main() {
    println!("{}", meridian_block_threshold(None));
}
