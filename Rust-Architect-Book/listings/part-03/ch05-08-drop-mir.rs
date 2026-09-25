// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target mir -Mode debug
pub fn two_names(a: &str, b: &str) -> usize {
    let first = a.to_string();
    let second = b.to_string(); // if this allocation panics, `first` must still be dropped
    first.len() + second.len()
}
