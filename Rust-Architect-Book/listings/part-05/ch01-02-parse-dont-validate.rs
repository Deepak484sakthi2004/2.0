// verify: debug ok
mod domain {
    use std::fmt;

    /// An e-mail address that has been checked. The field is private: the only way to get
    /// an `Email` is `Email::parse`, so every `Email` in the program is valid.
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct Email(String);

    #[derive(Debug, PartialEq)]
    pub enum EmailError {
        Empty,
        MissingAt,
        BadDomain,
    }

    impl Email {
        pub fn parse(raw: &str) -> Result<Email, EmailError> {
            let raw = raw.trim();
            if raw.is_empty() {
                return Err(EmailError::Empty);
            }
            let (local, domain) = raw.split_once('@').ok_or(EmailError::MissingAt)?;
            if local.is_empty() || !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') {
                return Err(EmailError::BadDomain);
            }
            Ok(Email(raw.to_ascii_lowercase()))
        }

        pub fn as_str(&self) -> &str {
            &self.0
        }
    }

    impl fmt::Display for Email {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.0)
        }
    }
}

use domain::{Email, EmailError};

/// Takes an `Email`, not a `&str`: this function cannot be handed an unchecked string.
fn send_receipt(to: &Email, order_id: u64) -> String {
    format!("receipt for order {order_id} queued to {to}")
}

fn main() {
    for raw in ["  Ada@Example.COM ", "", "ada.example.com", "ada@localhost", "grace@navy.mil"] {
        match Email::parse(raw) {
            Ok(email) => println!("{raw:?} -> {}", send_receipt(&email, 42)),
            Err(e) => println!("{raw:?} -> rejected: {e:?}"),
        }
    }
    assert_eq!(Email::parse("x@y"), Err(EmailError::BadDomain));
    let e = Email::parse("Ops@Meridian.Example").unwrap();
    println!("normalized once, at the boundary: {}", e.as_str());
}
