// verify: debug ok
// verify: debug@2021 ok
// The fix the compiler suggests: say which type you meant. Now no fallback is involved, and the
// program means the same thing in every edition.
fn load<T: Default>() -> Result<T, String> {
    Ok(T::default())
}

fn run() -> Result<(), String> {
    load::<()>()?;
    Ok(())
}

fn main() {
    println!("{:?}", run());
}
