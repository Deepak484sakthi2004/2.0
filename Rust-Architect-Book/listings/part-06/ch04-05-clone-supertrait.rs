// verify: debug error:E0038
struct Request {
    path: String,
}

/// "Every middleware must be cloneable so we can copy the pipeline per listener."
trait Middleware: Clone {
    fn handle(&self, req: &mut Request) -> Result<(), String>;
}

#[derive(Clone)]
struct Auth;

impl Middleware for Auth {
    fn handle(&self, req: &mut Request) -> Result<(), String> {
        if req.path.starts_with("/v1/") { Ok(()) } else { Err("404".into()) }
    }
}

fn main() {
    let pipeline: Vec<Box<dyn Middleware>> = vec![Box::new(Auth)];
    let mut req = Request { path: "/v1/payments".into() };
    println!("{:?}", pipeline[0].handle(&mut req));
}
