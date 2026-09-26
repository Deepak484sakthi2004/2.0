// verify: release build
// Why the harness (ch02-09) found the "same" sum 1.7x apart: inspect with tools/emit.ps1 -Target asm -Mode release.
#[inline(never)]
pub fn sum_iter(v: &Vec<u64>) -> u64 {
    v.iter().sum()
}

#[inline(never)]
pub fn sum_index(v: &Vec<u64>) -> u64 {
    let mut s = 0u64;
    for i in 0..v.len() {
        s += v[i];
    }
    s
}
