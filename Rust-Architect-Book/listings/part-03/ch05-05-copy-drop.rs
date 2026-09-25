// verify: debug error:E0184
#[derive(Clone, Copy)]
struct Handle(u32);

impl Drop for Handle {
    fn drop(&mut self) {
        println!("releasing handle {}", self.0);
    }
}

fn main() {
    let h = Handle(1);
    let _copy = h;
}
