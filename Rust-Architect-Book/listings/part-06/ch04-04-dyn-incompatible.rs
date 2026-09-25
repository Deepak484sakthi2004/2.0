// verify: debug error:E0038
use std::fmt::Debug;

struct Request {
    path: String,
    headers: Vec<(String, String)>,
}

trait Middleware {
    fn handle(&self, req: &mut Request) -> Result<(), String>;

    /// Added in v2.3 so every middleware can emit a typed metric. A GENERIC method.
    fn record<M: Debug>(&self, metric: M) {
        println!("metric: {metric:?}");
    }
}

struct Auth;

impl Middleware for Auth {
    fn handle(&self, req: &mut Request) -> Result<(), String> {
        if req.headers.iter().any(|(k, _)| k == "authorization") { Ok(()) } else { Err("401".into()) }
    }
}

fn main() {
    let pipeline: Vec<Box<dyn Middleware>> = vec![Box::new(Auth)];
    let mut req = Request { path: "/v1/payments".into(), headers: vec![] };
    for m in &pipeline {
        println!("{} -> {:?}", req.path, m.handle(&mut req));
    }
}
