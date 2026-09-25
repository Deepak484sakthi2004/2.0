// verify: debug error:E0603
mod domain {
    #[derive(Debug)]
    pub struct Email(String); // public type, private field

    impl Email {
        pub fn parse(raw: &str) -> Option<Email> {
            raw.contains('@').then(|| Email(raw.to_string()))
        }
    }
}

fn main() {
    let ok = domain::Email::parse("ada@example.com");
    let forged = domain::Email(String::from("not-an-email")); // bypass the parser?
    println!("{ok:?} {forged:?}");
}
