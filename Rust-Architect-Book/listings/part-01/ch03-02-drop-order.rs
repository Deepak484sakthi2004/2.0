// verify: debug ok
struct Connection {
    id: u32,
}

impl Drop for Connection {
    fn drop(&mut self) {
        println!("closing connection {}", self.id);
    }
}

fn main() {
    let a = Connection { id: 1 };
    {
        let _b = Connection { id: 2 };
        println!("inner scope ends");
    }
    let c = a; // ownership moves; `a` will NOT be dropped
    let _d = Connection { id: 3 };
    println!("main ends; `c` holds connection {}", c.id);
}
