// verify: debug ok
// Answer-key check for Chapter 18.5's debugging exercise: three fixes for the loop back-edge E0502.
fn owned(lines: &[&str]) -> Vec<String> {
    let mut buf = String::new();
    let mut methods = Vec::new();
    for line in lines {
        buf.clear();
        buf.push_str(line);
        methods.push(buf.split(' ').next().unwrap().to_string()); // one allocation per line
    }
    methods
}

fn borrow_source<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    // no buffer at all: borrow from the input, which outlives the result
    lines.iter().map(|l| l.split(' ').next().unwrap()).collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Method {
    Get,
    Post,
    Other,
}

fn classify(lines: &[&str]) -> Vec<Method> {
    let mut buf = String::new();
    let mut methods = Vec::new();
    for line in lines {
        buf.clear();
        buf.push_str(line);
        methods.push(match buf.split(' ').next() {
            Some("GET") => Method::Get,
            Some("POST") => Method::Post,
            _ => Method::Other,
        }); // store a value that borrows nothing
    }
    methods
}

fn main() {
    let lines = ["GET /pay", "GET /refund", "POST /pay"];
    println!("{:?}", owned(&lines));
    println!("{:?}", borrow_source(&lines));
    println!("{:?}", classify(&lines));
}
