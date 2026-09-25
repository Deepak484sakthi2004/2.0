// verify: debug ok
use std::any::type_name_of_val;
use std::mem::size_of_val;

#[derive(Default)]
struct Request {
    trace: Vec<&'static str>,
}

trait Middleware {
    fn handle(&self, req: &mut Request) -> Result<(), String>;
}

struct Auth;
struct RateLimit {
    per_sec: u32,
}
struct TenantTag;
struct GeoFence;

impl Middleware for Auth {
    fn handle(&self, req: &mut Request) -> Result<(), String> {
        req.trace.push("auth");
        Ok(())
    }
}
impl Middleware for RateLimit {
    fn handle(&self, req: &mut Request) -> Result<(), String> {
        req.trace.push(if self.per_sec > 0 { "ratelimit" } else { "ratelimit(off)" });
        Ok(())
    }
}
impl Middleware for TenantTag {
    fn handle(&self, req: &mut Request) -> Result<(), String> {
        req.trace.push("tenant");
        Ok(())
    }
}
impl Middleware for GeoFence {
    fn handle(&self, req: &mut Request) -> Result<(), String> {
        req.trace.push("geofence");
        Ok(())
    }
}

/// Static composition: the whole pipeline is ONE type, so every call is a direct (inlinable) call.
struct Stack<A, B> {
    first: A,
    rest: B,
}

impl<A: Middleware, B: Middleware> Middleware for Stack<A, B> {
    fn handle(&self, req: &mut Request) -> Result<(), String> {
        self.first.handle(req)?;
        self.rest.handle(req)
    }
}

/// The dynamic extension point: a list of plugins chosen at run time.
struct Plugins(Vec<Box<dyn Middleware>>);

impl Middleware for Plugins {
    fn handle(&self, req: &mut Request) -> Result<(), String> {
        for p in &self.0 {
            p.handle(req)?;
        }
        Ok(())
    }
}

fn run(name: &str, m: &dyn Middleware, size: usize, ty: &str) {
    let mut req = Request::default();
    m.handle(&mut req).unwrap();
    println!("{name:<8} {size:>3} bytes  {:?}", req.trace);
    println!("         type = {}", ty.replace("playground::", ""));
}

fn main() {
    let fixed = Stack { first: Auth, rest: Stack { first: RateLimit { per_sec: 500 }, rest: TenantTag } };

    let dynamic = Plugins(vec![Box::new(Auth), Box::new(RateLimit { per_sec: 500 }), Box::new(TenantTag)]);

    let hybrid = Stack {
        first: Auth,
        rest: Stack { first: RateLimit { per_sec: 500 }, rest: Plugins(vec![Box::new(TenantTag), Box::new(GeoFence)]) },
    };

    run("static", &fixed, size_of_val(&fixed), type_name_of_val(&fixed));
    run("dynamic", &dynamic, size_of_val(&dynamic), type_name_of_val(&dynamic));
    run("hybrid", &hybrid, size_of_val(&hybrid), type_name_of_val(&hybrid));
}
