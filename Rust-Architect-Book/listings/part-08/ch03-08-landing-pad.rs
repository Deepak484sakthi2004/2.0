// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target asm -Mode release
pub struct Guard(pub u32);

impl Drop for Guard {
    #[inline(never)]
    fn drop(&mut self) {
        std::hint::black_box(self.0);
    }
}

#[inline(never)]
pub fn may_panic(x: u32) -> u32 {
    if x == 0 {
        panic!("zero");
    }
    x
}

#[inline(never)]
pub fn with_guard(x: u32) -> u32 {
    let g = Guard(x);
    let r = may_panic(x);
    drop(g);
    r
}
