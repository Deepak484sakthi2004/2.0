// verify: debug error:E0502
//! An async fn's future captures every lifetime in its arguments: `fetch(&self)` returns a future
//! that borrows `self` until the future is dropped, whether or not it has been polled.
struct Svc {
    name: String,
}

impl Svc {
    async fn fetch(&self, id: u64) -> String {
        format!("{}#{id}", self.name)
    }
}

fn main() {
    let mut svc = Svc { name: "payments".into() };
    let fut = svc.fetch(1); // not polled yet, but it already holds `&svc`
    svc.name.push_str("-v2"); // mutation while the future's borrow is live
    drop(fut);
}
