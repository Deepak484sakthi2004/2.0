// verify: debug ok
//! The Chapter 3.5 transaction guard, with a commit that can fail.
use std::fmt;

struct Db {
    events: Vec<String>,
    fail_next_commit: Option<CommitFailure>,
}

#[derive(Debug, Clone, Copy)]
enum CommitFailure {
    /// The server said no (serialization conflict): nothing was applied, safe to retry the whole transaction.
    Conflict,
    /// The connection died after COMMIT was sent: the outcome is unknown.
    ConnectionLost,
}

/// Proof that a commit happened. Only `Tx::commit` can build one.
#[derive(Debug)]
struct Committed {
    rows: usize,
}

#[derive(Debug)]
enum CommitError {
    /// Rolled back: retrying the whole transaction is safe.
    RolledBack { reason: &'static str },
    /// May or may not have committed: reconcile before doing anything else.
    OutcomeUnknown,
}

impl fmt::Display for CommitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RolledBack { reason } => write!(f, "transaction rolled back: {reason}"),
            Self::OutcomeUnknown => write!(f, "commit outcome unknown: reconcile before retrying"),
        }
    }
}

impl std::error::Error for CommitError {}

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

    /// Consumes the guard whatever happens: on success, on a clean failure, and on an unknown outcome.
    /// The caller can never "try again" with a transaction in an undefined state.
    fn commit(mut self) -> Result<Committed, CommitError> {
        self.done = true; // from here on, Drop must not issue its own ROLLBACK
        let rows = self.pending.len();
        match self.db.fail_next_commit.take() {
            None => {
                self.db.events.push(format!("COMMIT ({rows} rows)"));
                Ok(Committed { rows })
            }
            Some(CommitFailure::Conflict) => {
                self.db.events.push("ROLLBACK (serialization conflict)".into());
                Err(CommitError::RolledBack { reason: "serialization conflict" })
            }
            Some(CommitFailure::ConnectionLost) => {
                self.db.events.push("COMMIT sent; connection lost".into());
                Err(CommitError::OutcomeUnknown)
            }
        }
    }
}

impl Drop for Tx<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.db.events.push("ROLLBACK (guard dropped)".into());
        }
    }
}

fn transfer(db: &mut Db) -> Result<Committed, CommitError> {
    let mut tx = db.begin();
    tx.insert("debit acct-1 100");
    tx.insert("credit acct-2 100");
    tx.commit()
}

fn main() {
    let mut db = Db { events: vec![], fail_next_commit: None };
    for failure in [None, Some(CommitFailure::Conflict), Some(CommitFailure::ConnectionLost)] {
        db.fail_next_commit = failure;
        match transfer(&mut db) {
            Ok(c) => println!("ok: committed {} rows", c.rows),
            Err(e @ CommitError::RolledBack { .. }) => println!("retry the transaction: {e}"),
            Err(e @ CommitError::OutcomeUnknown) => println!("do NOT blindly retry: {e}"),
        }
    }
    println!("{:?}", db.events);
}
