// verify: debug ok
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;

#[derive(Debug, Default)]
struct Request {
    path: String,
    headers: Vec<(String, String)>,
    tenant: Option<String>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// `Any` as a supertrait lets callers upcast &dyn Middleware to &dyn Any and downcast (Rust 1.86+).
trait Middleware: Any {
    fn name(&self) -> &'static str;
    fn handle(&self, req: &mut Request) -> Result<(), String>;

    /// Generic method kept OUT of the vtable: callable only on concrete (Sized) types.
    fn record<M: Debug>(&self, metric: M)
    where
        Self: Sized,
    {
        println!("  [{}] metric {metric:?}", self.name());
    }

    /// The dyn-compatible replacement for a `Clone` supertrait: returns a trait object, not `Self`.
    fn clone_box(&self) -> Box<dyn Middleware>;
}

#[derive(Clone)]
struct Auth;

#[derive(Clone)]
struct RateLimit {
    per_sec: u32,
}

#[derive(Clone)]
struct TenantTag;

impl Middleware for Auth {
    fn name(&self) -> &'static str {
        "auth"
    }
    fn handle(&self, req: &mut Request) -> Result<(), String> {
        req.header("authorization").map(|_| ()).ok_or_else(|| "401 missing credentials".to_string())
    }
    fn clone_box(&self) -> Box<dyn Middleware> {
        Box::new(self.clone())
    }
}

impl Middleware for RateLimit {
    fn name(&self) -> &'static str {
        "ratelimit"
    }
    fn handle(&self, _req: &mut Request) -> Result<(), String> {
        Ok(()) // the real limiter (Chapter 6.1) would try_acquire here
    }
    fn clone_box(&self) -> Box<dyn Middleware> {
        Box::new(self.clone())
    }
}

impl Middleware for TenantTag {
    fn name(&self) -> &'static str {
        "tenant"
    }
    fn handle(&self, req: &mut Request) -> Result<(), String> {
        req.tenant = req.header("x-tenant").map(str::to_string);
        Ok(())
    }
    fn clone_box(&self) -> Box<dyn Middleware> {
        Box::new(self.clone())
    }
}

/// A factory turns the config argument into a boxed middleware. Plain fn pointers: no captures needed.
type Factory = fn(&str) -> Box<dyn Middleware>;

fn registry() -> HashMap<&'static str, Factory> {
    let mut r: HashMap<&'static str, Factory> = HashMap::new();
    r.insert("auth", |_| Box::new(Auth));
    r.insert("ratelimit", |arg| Box::new(RateLimit { per_sec: arg.parse().unwrap_or(100) }));
    r.insert("tenant", |_| Box::new(TenantTag));
    r
}

/// Build the pipeline from a config string such as "auth, ratelimit=500, tenant".
fn build(config: &str, reg: &HashMap<&'static str, Factory>) -> Result<Vec<Box<dyn Middleware>>, String> {
    config
        .split(',')
        .map(|entry| {
            let (name, arg) = entry.split_once('=').unwrap_or((entry, ""));
            let name = name.trim();
            reg.get(name).map(|make| make(arg.trim())).ok_or_else(|| format!("unknown middleware {name:?}"))
        })
        .collect()
}

fn run(pipeline: &[Box<dyn Middleware>], req: &mut Request) -> Result<(), String> {
    for m in pipeline {
        m.handle(req).map_err(|e| format!("{}: {e}", m.name()))?;
    }
    Ok(())
}

fn main() {
    let reg = registry();
    let pipeline = build("auth, ratelimit=500, tenant", &reg).unwrap();
    let names: Vec<&str> = pipeline.iter().map(|m| m.name()).collect();
    println!("pipeline = {names:?}");

    let mut req = Request {
        path: "/v1/payments".into(),
        headers: vec![("authorization".into(), "Bearer t0k".into()), ("x-tenant".into(), "acme".into())],
        tenant: None,
    };
    let outcome = run(&pipeline, &mut req);
    println!("{} -> {outcome:?}, tenant = {:?}", req.path, req.tenant);

    let mut anonymous = Request { path: "/v1/payments".into(), ..Default::default() };
    let outcome = run(&pipeline, &mut anonymous);
    println!("{} -> {outcome:?}", anonymous.path);

    println!("bad config -> {:?}", build("auth, geoip", &reg).err());

    // The generic method works on a concrete type; `pipeline[0].record(..)` would not compile.
    Auth.record(("latency_us", 412));

    // clone_box: an independent copy of the pipeline for a second listener.
    let copy: Vec<Box<dyn Middleware>> = pipeline.iter().map(|m| m.clone_box()).collect();
    println!("copied {} middleware", copy.len());

    // Upcast &dyn Middleware -> &dyn Any, then downcast to a concrete type.
    for m in &pipeline {
        let any: &dyn Any = &**m;
        if let Some(rl) = any.downcast_ref::<RateLimit>() {
            println!("rate limit configured at {}/s", rl.per_sec);
        }
    }
}
