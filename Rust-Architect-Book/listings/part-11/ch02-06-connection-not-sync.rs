// verify: debug error:E0277
//! Libraries encode their threading contract in Send/Sync. rusqlite's Connection is Send (move it to
//! another thread) but not Sync (never use it from two threads at once): it caches statements in a RefCell.
use rusqlite::Connection;
use std::thread;

fn main() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE audit (id INTEGER PRIMARY KEY, what TEXT)").unwrap();
    thread::scope(|s| {
        for i in 0..2 {
            let conn = &conn; // sharing &Connection requires Connection: Sync
            s.spawn(move || {
                conn.execute("INSERT INTO audit (what) VALUES (?1)", [format!("worker {i}")]).unwrap();
            });
        }
    });
}
