// verify: release error:E0133
// Since Rust 1.86, a #[target_feature] function can be a safe `fn`, but calling it from code that doesn't have the
// feature enabled still requires `unsafe`: the caller must prove the CPU supports it.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn fast_path(v: &[u8]) -> usize {
    v.len()
}

fn main() {
    let v = [1u8, 2, 3];
    println!("{}", fast_path(&v)); // no runtime check, no `unsafe`: rejected
}
