// verify: debug ok
// The macro's `t` and the caller's `t` are different identifiers to the compiler (hygiene), even
// though the pretty-printed expansion (-Target expand) shows both as `t`.
macro_rules! scaled {
    ($e:expr) => {{
        let t = 2; // the macro's own `t`
        t * $e
    }};
}

fn main() {
    let t = 10; // the caller's `t`
    let r = scaled!(t + 1);
    println!("r = {r}");
}
