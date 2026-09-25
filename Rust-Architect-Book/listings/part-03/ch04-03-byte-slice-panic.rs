// verify: debug panic is not a char boundary
fn display_name(full: &str) -> &str {
    // BUG: 10 is a BYTE index, not a character count
    if full.len() > 10 { &full[..10] } else { full }
}

fn main() {
    println!("{}", display_name("Ada Lovelace"));
    println!("{}", display_name("Kristina Øberg"));
}
