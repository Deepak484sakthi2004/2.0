// verify: debug ok
//! The owner-thread ("actor") pattern: one thread owns the HashMap outright; everyone else sends it
//! commands. No lock anywhere, and every operation on the map is naturally serialized.
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::{self, JoinHandle};

#[derive(Clone, Debug)]
pub struct Session {
    pub user: String,
    pub hits: u32,
}

enum Command {
    Touch { id: u64, user: String },
    Get { id: u64, reply: SyncSender<Option<Session>> },
    Expire { max_hits: u32, reply: SyncSender<usize> },
}

#[derive(Clone)]
pub struct SessionStore {
    tx: Sender<Command>,
}

impl SessionStore {
    pub fn start() -> (SessionStore, JoinHandle<usize>) {
        let (tx, rx) = mpsc::channel();
        let owner = thread::Builder::new().name("session-owner".into()).spawn(move || run(rx)).unwrap();
        (SessionStore { tx }, owner)
    }

    pub fn touch(&self, id: u64, user: &str) {
        self.tx.send(Command::Touch { id, user: user.to_string() }).expect("owner thread gone");
    }

    pub fn get(&self, id: u64) -> Option<Session> {
        let (reply, answer) = mpsc::sync_channel(1); // a one-shot reply channel per request
        self.tx.send(Command::Get { id, reply }).expect("owner thread gone");
        answer.recv().expect("owner dropped the reply")
    }

    pub fn expire(&self, max_hits: u32) -> usize {
        let (reply, answer) = mpsc::sync_channel(1);
        self.tx.send(Command::Expire { max_hits, reply }).expect("owner thread gone");
        answer.recv().expect("owner dropped the reply")
    }
}

/// The owner: plain `&mut HashMap`, no synchronization inside. Returns the number of commands handled.
fn run(rx: Receiver<Command>) -> usize {
    let mut sessions: HashMap<u64, Session> = HashMap::new();
    let mut handled = 0;
    for cmd in rx {
        handled += 1;
        match cmd {
            Command::Touch { id, user } => sessions.entry(id).or_insert(Session { user, hits: 0 }).hits += 1,
            Command::Get { id, reply } => {
                let _ = reply.send(sessions.get(&id).cloned()); // the caller may have given up: ignore
            }
            Command::Expire { max_hits, reply } => {
                let before = sessions.len();
                sessions.retain(|_, s| s.hits > max_hits);
                let _ = reply.send(before - sessions.len());
            }
        }
    }
    handled // every SessionStore handle was dropped: the loop ends, the map is dropped here
}

fn main() {
    let (store, owner) = SessionStore::start();
    thread::scope(|s| {
        for w in 0..4u64 {
            let store = store.clone();
            s.spawn(move || {
                for i in 0..100 {
                    let id = i % 10 + w * 100; // 10 sessions per worker
                    store.touch(id, &format!("user{id}"));
                }
            });
        }
    });
    println!("session 3: {:?}", store.get(3));
    store.touch(999, "one-off");
    println!("expired sessions with <= 1 hit: {}", store.expire(1));
    println!("session 999 after expiry: {:?}", store.get(999));
    drop(store); // the last handle: the owner's loop ends
    println!("owner thread handled {} commands, then exited", owner.join().unwrap());
}
