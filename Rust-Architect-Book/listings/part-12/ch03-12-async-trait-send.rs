// verify: debug error:E0277
//! The "Send bound problem": generic code can't require that a trait's `async fn` future is Send,
//! because the future type has no name to put a bound on (stable Rust 1.98; see the chapter).
trait Fetch {
    async fn fetch(&self, id: u64) -> String;
}

fn spawn_fetch<F: Fetch + Send + Sync + 'static>(f: &'static F) {
    // Spawning onto another thread needs the future to be Send, and we can't say so.
    let fut = f.fetch(1);
    std::thread::spawn(move || futures::executor::block_on(fut));
}

struct Db;
impl Fetch for Db {
    async fn fetch(&self, id: u64) -> String {
        format!("row {id}")
    }
}

fn main() {
    static DB: Db = Db;
    spawn_fetch(&DB);
}
