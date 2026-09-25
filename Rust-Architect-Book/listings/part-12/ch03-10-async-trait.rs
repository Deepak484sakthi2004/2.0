// verify: debug ok
//! async fn in traits (stable since 1.75): static dispatch, the `-> impl Future + Send` form that
//! lets a trait promise Send futures, and the boxed-future pattern that makes a dyn-compatible trait.
use std::future::Future;
use std::pin::Pin;

// 1. Static dispatch: each impl's `fetch` returns its own anonymous future type.
trait Fetch {
    async fn fetch(&self, id: u64) -> String;
}

// 2. The same method, desugared by hand, so that the trait can PROMISE callers a Send future.
//    Implementations may still write `async fn`; the compiler checks that their future is Send.
trait FetchSend {
    fn fetch(&self, id: u64) -> impl Future<Output = String> + Send;
}

// 3. dyn-compatible: every impl returns the same concrete type, a boxed, type-erased future.
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
trait DynFetch: Send + Sync {
    fn fetch(&self, id: u64) -> BoxFuture<'_, String>;
}

struct Db {
    table: &'static str,
}

impl Fetch for Db {
    async fn fetch(&self, id: u64) -> String {
        format!("{}#{id}", self.table)
    }
}

impl FetchSend for Db {
    async fn fetch(&self, id: u64) -> String {
        format!("{}#{id} (Send)", self.table)
    }
}

// Every FetchSend type is a DynFetch too: the adapter costs one heap allocation per call.
impl<T: FetchSend + Send + Sync> DynFetch for T {
    fn fetch(&self, id: u64) -> BoxFuture<'_, String> {
        Box::pin(FetchSend::fetch(self, id))
    }
}

/// Generic code over the static trait: monomorphized per implementation, no boxing.
async fn fetch_both<F: Fetch>(f: &F) -> (String, String) {
    (f.fetch(1).await, f.fetch(2).await)
}

fn main() {
    let db = Db { table: "payments" };
    futures::executor::block_on(async {
        println!("{:?}", fetch_both(&db).await);
        println!("{}", FetchSend::fetch(&db, 8).await);
        let backends: Vec<Box<dyn DynFetch>> = vec![Box::new(Db { table: "refunds" }), Box::new(Db { table: "payouts" })];
        for b in &backends {
            println!("{}", b.fetch(9).await);
        }
    });

    // The Send promise is what allows moving the future to another thread.
    let fut = async move { FetchSend::fetch(&Db { table: "ledger" }, 10).await };
    println!("{}", std::thread::spawn(move || futures::executor::block_on(fut)).join().unwrap());
}
