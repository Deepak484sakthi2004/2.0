// verify: debug error:E0107
use std::fmt::Display;

/// Argument-position impl Trait: an anonymous type parameter the caller cannot name.
fn log_field(name: &str, value: impl Display) -> String {
    format!("{name}={value}")
}

fn main() {
    println!("{}", log_field("port", 8080));
    println!("{}", log_field::<u16>("port", 8080)); // there is no parameter you can name here
}
