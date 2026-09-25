// verify: debug build
// Source for the HIR artifact in Chapter 18.2 (tools/emit.ps1 -Target hir): `for`, `?`, `while let`,
// `async fn`, and `.await` as the compiler rewrites them during AST -> HIR lowering.
pub fn total(xs: &[u32]) -> u32 {
    let mut sum = 0;
    for x in xs {
        sum += x;
    }
    sum
}

pub fn parse_pair(a: &str, b: &str) -> Result<u32, std::num::ParseIntError> {
    let x: u32 = a.parse()?;
    Ok(x + b.len() as u32)
}

pub fn drain(stack: &mut Vec<u32>) -> u32 {
    let mut n = 0;
    while let Some(top) = stack.pop() {
        n += top;
    }
    n
}

pub async fn fetch(id: u32) -> u32 {
    id + 1
}

pub async fn caller() -> u32 {
    fetch(7).await
}
