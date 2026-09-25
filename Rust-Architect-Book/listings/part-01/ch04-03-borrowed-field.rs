// verify: debug error:E0106
struct Request {
    method: String,
    path: &str,
}

fn main() {
    let raw = String::from("GET /health");
    let (method, path) = raw.split_once(' ').unwrap();
    let req = Request { method: method.to_string(), path };
    println!("{} {}", req.method, req.path);
}
