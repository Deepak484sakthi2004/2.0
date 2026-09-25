// verify: debug error:E0277
// verify: debug@2021 error:dependency_on_unit_never_type_fallback
// Nothing constrains T in `load()?;`, so inference FALLBACK decides it. Edition 2021 used `()`
// (and 1.98.1 already denies relying on that); edition 2024 uses `!`, which isn't Default.
fn load<T: Default>() -> Result<T, String> {
    Ok(T::default())
}

fn run() -> Result<(), String> {
    load()?; // T is never constrained: only the fallback decides it
    Ok(())
}

fn main() {
    println!("{:?}", run());
}
