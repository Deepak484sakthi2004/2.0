// verify: debug build
// Source for the second HIR artifact in Chapter 18.2: a range, an if-let chain (edition 2024),
// and println!/format_args!, whose final form is produced by AST -> HIR lowering.
pub fn report(limit: Option<u32>, n: u32) {
    let window = 0..n;
    if let Some(l) = limit
        && l < n
    {
        println!("limit {l} below {}", window.end);
    }
}
