// verify: debug error:E0080
/// The one place a generic body is checked PER INSTANTIATION: constant evaluation.
fn first_byte<const N: usize>(block: [u8; N]) -> u8 {
    const { assert!(N > 0, "first_byte needs a non-empty block") };
    block[0]
}

fn main() {
    println!("{}", first_byte([7, 8, 9])); // N = 3: the assertion holds
    println!("{}", first_byte([])); // N = 0: fails while instantiating first_byte::<0>
}
