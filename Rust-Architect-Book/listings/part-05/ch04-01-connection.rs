// verify: debug ok
mod db {
    /// A stand-in for a TCP stream (the Playground has no network; the states are the point).
    #[derive(Debug)]
    pub struct Socket {
        fd: i32,
    }

    #[derive(Debug)]
    pub struct SessionToken(String);

    // The three states. Each carries exactly the data that exists in that state.
    #[derive(Debug)]
    pub struct Disconnected;
    #[derive(Debug)]
    pub struct Connected {
        socket: Socket,
    }
    #[derive(Debug)]
    pub struct Authenticated {
        socket: Socket,
        session: SessionToken,
    }

    #[derive(Debug)]
    pub struct Connection<S> {
        addr: String,
        state: S,
    }

    #[derive(Debug)]
    pub enum ConnectError {
        Refused,
    }
    #[derive(Debug)]
    pub enum AuthError {
        BadCredentials,
    }

    // Available in every state.
    impl<S> Connection<S> {
        pub fn addr(&self) -> &str {
            &self.addr
        }
    }

    impl Connection<Disconnected> {
        pub fn new(addr: &str) -> Self {
            Connection { addr: addr.to_string(), state: Disconnected }
        }

        /// Consumes the disconnected handle. On failure, hands it back so the caller can retry.
        pub fn connect(self) -> Result<Connection<Connected>, (Connection<Disconnected>, ConnectError)> {
            if self.addr.ends_with(":0") {
                return Err((self, ConnectError::Refused));
            }
            let socket = Socket { fd: 3 };
            Ok(Connection { addr: self.addr, state: Connected { socket } })
        }
    }

    impl Connection<Connected> {
        pub fn authenticate(self, user: &str, password: &str) -> Result<Connection<Authenticated>, (Connection<Connected>, AuthError)> {
            if password.len() < 8 {
                return Err((self, AuthError::BadCredentials));
            }
            let session = SessionToken(format!("sess-{user}-{}", self.state.socket.fd));
            Ok(Connection { addr: self.addr, state: Authenticated { socket: self.state.socket, session } })
        }

        pub fn close(self) -> Connection<Disconnected> {
            Connection { addr: self.addr, state: Disconnected }
        }
    }

    impl Connection<Authenticated> {
        /// Only an authenticated connection can run queries: there is no "not authenticated" error path.
        pub fn query(&mut self, sql: &str) -> Vec<String> {
            vec![format!("[{} via fd {}] {sql}", self.state.session.0, self.state.socket.fd)]
        }

        pub fn close(self) -> Connection<Disconnected> {
            Connection { addr: self.addr, state: Disconnected }
        }
    }
}

use db::{Connection, Disconnected};

fn main() {
    // A refused connection returns the handle, so we can retry with another address.
    let conn = Connection::<Disconnected>::new("10.0.0.5:0");
    let conn = match conn.connect() {
        Ok(_) => unreachable!(),
        Err((back, e)) => {
            println!("connect to {} failed: {e:?}; retrying", back.addr());
            Connection::new("10.0.0.5:5432")
        }
    };

    let conn = conn.connect().expect("reachable");
    let conn = match conn.authenticate("ledger", "short") {
        Ok(_) => unreachable!(),
        Err((back, e)) => {
            println!("auth failed: {e:?}; still connected to {}", back.addr());
            back
        }
    };
    let mut conn = conn.authenticate("ledger", "correct-horse").expect("valid credentials");
    for row in conn.query("SELECT balance FROM accounts WHERE id = 7") {
        println!("{row}");
    }
    let conn: Connection<Disconnected> = conn.close();
    println!("closed: {conn:?}");
    let idle = Connection::new("10.0.0.6:5432").connect().expect("reachable").close();
    println!("connected and closed without authenticating: {}", idle.addr());
    println!(
        "size_of: Connection<Disconnected>={} Connection<Authenticated>={}",
        std::mem::size_of::<Connection<Disconnected>>(),
        std::mem::size_of::<db::Connection<db::Authenticated>>()
    );
}
