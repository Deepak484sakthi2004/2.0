// verify: debug ok
// The Part IV review capstone, fixed. Each fix names the ownership argument it resolves.
use std::collections::HashMap;

struct Session {
    user: String,
    hits: u32,
}

struct Manager {
    sessions: HashMap<u64, Session>,
    log: Vec<String>,
}

impl Manager {
    /// Fix A (split borrow was already fine here; the loan is on `self.sessions` only):
    /// return a copy of what the caller needs instead of a reference that locks the whole manager.
    fn touch(&mut self, id: u64) -> u32 {
        let s = self.sessions.get_mut(&id).expect("known session");
        s.hits += 1;
        let hits = s.hits;
        self.log.push(format!("touch {id}"));
        hits
    }

    /// Fix B: iteration + removal in one purpose-built call.
    fn expire_idle(&mut self) -> usize {
        let before = self.sessions.len();
        self.sessions.retain(|_, s| s.hits > 0);
        before - self.sessions.len()
    }

    /// Fix C: move the old value OUT instead of borrowing it, and return it owned.
    fn rename(&mut self, id: u64, user: &str) -> String {
        let s = self.sessions.get_mut(&id).expect("known session");
        std::mem::replace(&mut s.user, user.to_string())
    }
}

fn main() {
    let mut m = Manager { sessions: HashMap::new(), log: Vec::new() };
    m.sessions.insert(1, Session { user: "ada".into(), hits: 0 });
    m.sessions.insert(2, Session { user: "grace".into(), hits: 0 });

    let hits = m.touch(1);
    let expired = m.expire_idle();
    println!("session 1 has {hits} hit(s); expired {expired} idle session(s)");

    let old = m.rename(1, "ada.l");
    println!("renamed {old} -> {}; log = {:?}", m.sessions[&1].user, m.log);
}
