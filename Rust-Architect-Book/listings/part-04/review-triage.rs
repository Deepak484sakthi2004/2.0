// verify: debug error:E0502
// The Part IV review capstone: a session manager with several borrow-checker errors.
// Each one is a different ownership argument. (All errors are reported in one compile.)
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
    fn touch(&mut self, id: u64) -> &Session {
        let s = self.sessions.get_mut(&id).expect("known session");
        s.hits += 1;
        self.log.push(format!("touch {id}"));
        s
    }

    fn expire_idle(&mut self) {
        for (id, s) in &self.sessions {
            if s.hits == 0 {
                self.sessions.remove(id);
            }
        }
    }

    fn rename(&mut self, id: u64, user: &str) -> &str {
        let s = self.sessions.get_mut(&id).expect("known session");
        let old = &s.user;
        s.user = user.to_string();
        old
    }
}

fn main() {
    let mut m = Manager { sessions: HashMap::new(), log: Vec::new() };
    m.sessions.insert(1, Session { user: "ada".into(), hits: 0 });
    m.sessions.insert(2, Session { user: "grace".into(), hits: 0 });

    let s = m.touch(1);
    m.expire_idle();
    println!("{} has {} hit(s)", s.user, s.hits);

    let old = m.rename(1, "ada.l");
    println!("renamed {old}; log = {:?}", m.log);
}
