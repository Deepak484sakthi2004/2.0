// verify: debug ok
use std::panic::{self, AssertUnwindSafe};

struct Db {
    committed: Vec<String>,
    events: Vec<String>,
}

/// A transaction borrows the database exclusively for its whole life.
struct Tx<'db> {
    db: &'db mut Db,
    pending: Vec<String>,
    done: bool,
}

impl Db {
    fn begin(&mut self) -> Tx<'_> {
        self.events.push("BEGIN".into());
        Tx { db: self, pending: Vec::new(), done: false }
    }
}

impl Tx<'_> {
    fn insert(&mut self, row: &str) {
        self.pending.push(row.to_string());
    }

    /// Consumes the transaction: after commit, it cannot be used (or committed) again.
    fn commit(mut self) {
        self.db.committed.append(&mut self.pending);
        self.db.events.push("COMMIT".into());
        self.done = true;
    } // `self` is dropped here, and Drop sees done == true
}

impl Drop for Tx<'_> {
    fn drop(&mut self) {
        if !self.done {
            let discarded = self.pending.len();
            self.db.events.push(format!("ROLLBACK ({discarded} pending row(s) discarded)"));
        }
    }
}

fn transfer(db: &mut Db, fail: bool) -> Result<(), String> {
    let mut tx = db.begin();
    tx.insert("debit acct-1 100");
    if fail {
        return Err("credit side rejected".into()); // early return: `tx` dropped → rollback
    }
    tx.insert("credit acct-2 100");
    tx.commit();
    Ok(())
}

fn main() {
    let mut db = Db { committed: vec![], events: vec![] };
    println!("{:?}", transfer(&mut db, false));
    println!("{:?}", transfer(&mut db, true));

    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let mut tx = db.begin();
        tx.insert("orphan row");
        panic!("bug while building the transaction"); // unwinding drops `tx` → rollback
    }));
    println!("panicked: {}", result.is_err());
    println!("events:    {:?}", db.events);
    println!("committed: {:?}", db.committed);
}
