// verify: debug ok
// Listing 17.4-6: three resolution rules of Rust, observed.
//   1. Items are visible in their whole module, before or after their definition.
//   2. Types and values live in different namespaces (`i32` can name both).
//   3. macro_rules! is hygienic for local variables: a macro's `x` is not the caller's `x`.
//      (Items a macro defines are visible to the caller: that hygiene covers locals and labels.)

macro_rules! double_it {
    ($e:expr) => {{
        let x = 2; // the macro's own `x`
        x * $e     // `$e` still refers to the caller's `x`
    }};
}

macro_rules! make_fn {
    ($name:ident) => {
        fn $name() -> u32 {
            42
        }
    };
}

make_fn!(answer);

fn main() {
    println!("used before its definition: {}", later(1));

    let i32 = 7_i32; // a VALUE named i32...
    let n: i32 = i32 * 2; // ...next to the TYPE i32
    println!("value namespace vs type namespace: {n}");

    let x = 10;
    println!("hygiene: double_it!(x) = {} (unhygienic textual expansion would give 4)", double_it!(x));
    println!("an item defined by a macro: answer() = {}", answer());

    let v = 1;
    let v = v + 1; // the initializer sees the OLD `v`
    println!("shadowing: v = {v}");
}

fn later(a: i32) -> i32 {
    a + 100
}
