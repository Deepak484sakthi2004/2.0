// verify: debug@2021 ok
// verify: debug error:reserved
fn main() {
    let gen = 5; // fine in edition 2021; `gen` is a reserved keyword in edition 2024
    println!("{gen}");
}
