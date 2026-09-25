// verify: debug ok
// Fix 1: own the data. Simple to hold anywhere; costs an allocation + copy per field.
struct Request {
    method: String,
    path: String,
}

// Fix 2: borrow from the input buffer. Zero-copy; but the lifetime 'a now
// appears in every type and function signature that holds a RequestRef.
struct RequestRef<'a> {
    method: &'a str,
    path: &'a str,
}

fn parse(raw: &str) -> Option<RequestRef<'_>> {
    let (method, path) = raw.split_once(' ')?;
    Some(RequestRef { method, path })
}

fn main() {
    let raw = String::from("GET /health");
    let borrowed = parse(&raw).expect("well-formed request line");
    let owned = Request {
        method: borrowed.method.to_string(),
        path: borrowed.path.to_string(),
    };
    println!("borrowed: {} {}", borrowed.method, borrowed.path);
    println!("owned:    {} {}", owned.method, owned.path);
}
