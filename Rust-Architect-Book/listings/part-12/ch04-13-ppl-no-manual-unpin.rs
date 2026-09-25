// verify: debug error:E0119
//! ch04-06's mistake, attempted on the pin-project-lite version: the macro already generates a
//! conditional `impl Unpin`, so a manual unconditional one conflicts and doesn't compile.
pin_project_lite::pin_project! {
    struct WithBudget<F> {
        #[pin]
        inner: F,
        polls_left: u32,
    }
}

impl<F> Unpin for WithBudget<F> {}

fn main() {}
