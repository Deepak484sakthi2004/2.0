// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target mir -Mode debug
// What a closure lowers to: a struct of captures plus a method that takes it by &self / &mut self / self.
#[inline(never)]
pub fn count_over(values: &[u64], threshold: u64) -> usize {
    let mut seen = 0usize;
    let mut check = |v: u64| {
        if v > threshold {
            seen += 1;
        }
    };
    for &v in values {
        check(v);
    }
    seen
}
