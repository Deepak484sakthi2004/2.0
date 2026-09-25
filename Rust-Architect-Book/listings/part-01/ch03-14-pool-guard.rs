// verify: debug ok
use std::cell::RefCell;
use std::panic::{self, AssertUnwindSafe};

struct Pool {
    idle: RefCell<Vec<u32>>,
}

// A checked-out connection. It borrows the pool, so it cannot outlive it.
struct PooledConn<'p> {
    id: u32,
    pool: &'p Pool,
}

impl Pool {
    fn get(&self) -> Option<PooledConn<'_>> {
        let id = self.idle.borrow_mut().pop()?;
        Some(PooledConn { id, pool: self })
    }
}

impl Drop for PooledConn<'_> {
    fn drop(&mut self) {
        self.pool.idle.borrow_mut().push(self.id); // runs on EVERY exit path
    }
}

fn handle(pool: &Pool, fail: bool) -> Result<(), String> {
    let conn = pool.get().ok_or("pool exhausted")?;
    if fail {
        return Err(format!("query on conn {} failed", conn.id)); // early return: still returned
    }
    Ok(())
}

fn main() {
    let pool = Pool { idle: RefCell::new(vec![1, 2]) };

    let failures = (0..10).filter(|i| handle(&pool, i % 3 == 0).is_err()).count();
    println!("requests: 10, failed: {failures}");

    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let _conn = pool.get().unwrap();
        panic!("bug in handler"); // unwinding runs _conn's destructor
    }));
    println!("handler panicked: {}", result.is_err());
    println!("idle connections: {}", pool.idle.borrow().len());
}
