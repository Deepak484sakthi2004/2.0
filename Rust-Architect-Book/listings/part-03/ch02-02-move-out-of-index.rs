// verify: debug error:E0507
fn main() {
    let names = vec![String::from("ada"), String::from("grace")];
    let first = names[0];
    println!("{first} {}", names.len());
}
