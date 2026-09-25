// verify: debug error:E0277
use std::thread;

trait Middleware {
    fn handle(&self, path: &str) -> bool;
}

struct Auth;

impl Middleware for Auth {
    fn handle(&self, path: &str) -> bool {
        path.starts_with("/v1/")
    }
}

fn main() {
    let pipeline: Vec<Box<dyn Middleware>> = vec![Box::new(Auth)];
    let worker = thread::spawn(move || pipeline.iter().all(|m| m.handle("/v1/payments")));
    println!("{:?}", worker.join().unwrap());
}
