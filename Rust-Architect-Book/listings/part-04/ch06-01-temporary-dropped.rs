// verify: debug error:E0716
fn main() {
    // OK: `let x = &temporary;` gets TEMPORARY LIFETIME EXTENSION: the String lives as long as `tenant`.
    let tenant: &str = &format!("tenant-{}", 42);
    // E0716: here the temporary String is the receiver of a method call. It is NOT extended,
    // so it is dropped at the end of this statement, and `trimmed` would point into freed memory.
    let trimmed: &str = String::from("  acme  ").trim();
    println!("{tenant} {trimmed}");
}
