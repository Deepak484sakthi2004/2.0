// verify: debug error:E0382
struct Connection {
    id: u32,
}

impl Connection {
    fn close(self) {
        // takes ownership: the connection cannot be used after this
        println!("closing connection {}", self.id);
    }
}

fn main() {
    let conn = Connection { id: 7 };
    conn.close();
    conn.close();
}
