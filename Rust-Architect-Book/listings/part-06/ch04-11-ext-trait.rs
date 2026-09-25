// verify: debug ok
use std::fmt::Debug;

/// The dyn-compatible core: only what the vtable needs.
trait Middleware {
    fn name(&self) -> &'static str;
    fn handle(&self, path: &str) -> bool;
}

/// Generic conveniences live in an EXTENSION trait with a blanket impl.
/// `?Sized` makes the blanket cover `dyn Middleware` itself, so trait objects get the generic methods too.
trait MiddlewareExt: Middleware {
    fn record<M: Debug>(&self, metric: M) {
        println!("[{}] metric {metric:?}", self.name());
    }
}

impl<T: Middleware + ?Sized> MiddlewareExt for T {}

/// Compile-time guard: this stops compiling (E0038) if anyone makes Middleware dyn-incompatible.
fn _assert_dyn_compatible(_: &dyn Middleware) {}

struct Auth;
struct Tenant;

impl Middleware for Auth {
    fn name(&self) -> &'static str {
        "auth"
    }
    fn handle(&self, path: &str) -> bool {
        path.starts_with("/v1/")
    }
}

impl Middleware for Tenant {
    fn name(&self) -> &'static str {
        "tenant"
    }
    fn handle(&self, _path: &str) -> bool {
        true
    }
}

fn main() {
    let pipeline: Vec<Box<dyn Middleware>> = vec![Box::new(Auth), Box::new(Tenant)];
    for m in &pipeline {
        let ok = m.handle("/v1/payments");
        m.record(("admitted", ok)); // generic method, called on a trait object
    }
    Auth.record(42u32); // and on a concrete type
}
