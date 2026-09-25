// verify: debug error:E0599
pub struct Disconnected;
pub struct Connected;
pub struct Authenticated;

pub struct Connection<S> {
    addr: String,
    state: S,
}

impl Connection<Disconnected> {
    pub fn new(addr: &str) -> Self {
        Connection { addr: addr.to_string(), state: Disconnected }
    }
    pub fn connect(self) -> Connection<Connected> {
        Connection { addr: self.addr, state: Connected }
    }
}

impl Connection<Connected> {
    pub fn authenticate(self, _password: &str) -> Connection<Authenticated> {
        Connection { addr: self.addr, state: Authenticated }
    }
}

impl Connection<Authenticated> {
    pub fn query(&mut self, sql: &str) -> String {
        format!("{}: {sql}", self.addr)
    }
}

fn main() {
    let mut conn = Connection::new("10.0.0.5:5432").connect();
    // Forgot to authenticate:
    println!("{}", conn.query("SELECT 1"));
}
