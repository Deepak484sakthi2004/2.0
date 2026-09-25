// verify: debug error:E0038
//! A trait with an `async fn` can't be used as `dyn Trait`: every implementation's method returns
//! a different (anonymous) future type, so no single vtable signature exists.
trait Fetch {
    async fn fetch(&self, id: u64) -> String;
}

struct Db;

impl Fetch for Db {
    async fn fetch(&self, id: u64) -> String {
        format!("row {id}")
    }
}

fn main() {
    let backend: Box<dyn Fetch> = Box::new(Db);
    let _ = backend;
}
