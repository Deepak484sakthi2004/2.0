// verify: debug crash panic in a destructor during cleanup
struct Connection {
    id: u32,
}

impl Drop for Connection {
    fn drop(&mut self) {
        println!("closing connection {}", self.id);
        // A fallible close that panics instead of reporting: fine on the normal path, fatal during unwinding.
        panic!("close failed for connection {}", self.id);
    }
}

fn main() {
    let _conn = Connection { id: 7 };
    panic!("request handler bug"); // unwinding drops _conn → its Drop panics → abort
}
