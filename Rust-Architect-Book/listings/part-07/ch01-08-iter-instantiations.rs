// verify: debug build
// Chapter 1.3's two functions, revisited. Inspect with:
//   tools\emit.ps1 <this file> -Target llvm-ir -Mode debug     (every monomorphized piece is a separate function)
//   tools\emit.ps1 <this file> -Target llvm-ir -Mode release   (inlining folds them together)
#[inline(never)]
pub fn sum_even_squares_loop(data: &[u64]) -> u64 {
    let mut total = 0;
    for i in 0..data.len() {
        let x = data[i];
        if x % 2 == 0 {
            total += x * x;
        }
    }
    total
}

#[inline(never)]
pub fn sum_even_squares_iter(data: &[u64]) -> u64 {
    data.iter().filter(|&&x| x % 2 == 0).map(|&x| x * x).sum()
}
