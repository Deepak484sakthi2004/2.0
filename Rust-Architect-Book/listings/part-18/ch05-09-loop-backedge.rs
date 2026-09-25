// verify: debug error:E0502
// A loan created in one iteration must stay valid at every point where something holding it is live.
// `methods` is live after the loop, so the loan on `buf` flows around the back edge into `buf.clear()`.
fn main() {
    let lines = ["GET /pay", "GET /refund", "POST /pay"];
    let mut buf = String::new();
    let mut methods: Vec<&str> = Vec::new();
    for line in lines {
        buf.clear();
        buf.push_str(line);
        let method = buf.split(' ').next().unwrap();
        methods.push(method);
    }
    println!("{methods:?}");
}
