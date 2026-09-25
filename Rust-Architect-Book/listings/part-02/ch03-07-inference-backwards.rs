// verify: debug ok
fn main() {
    let mut ids = Vec::new(); // Vec<?T>: the element type is not known yet
    ids.push(7u16); // ...now it is: ?T = u16
    let first = ids[0];
    let doubled = first * 2; // u16 arithmetic
    println!("{doubled} ({})", std::any::type_name_of_val(&ids));
}
