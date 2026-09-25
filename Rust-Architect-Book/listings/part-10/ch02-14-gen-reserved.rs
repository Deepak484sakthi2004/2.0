// verify: debug error:reserved
// verify: debug@2021 ok
fn main() {
    let gen = 7; // fine in edition 2021; `gen` is a reserved keyword in edition 2024
    println!("generation {gen}");
}
