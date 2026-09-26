// verify: debug error:improper_ctypes_definitions
// The PR's `init` under the CI setting Meridian uses for FFI crates (the lint denied). The PR's own
// CI job didn't deny it, so the warning scrolled past in the build log.
#![deny(improper_ctypes_definitions)]

#[unsafe(no_mangle)]
pub extern "C" fn init(model_name: String) -> bool {
    !model_name.is_empty()
}

fn main() {
    println!("{}", init("fraud-2026-09".to_string()));
}
