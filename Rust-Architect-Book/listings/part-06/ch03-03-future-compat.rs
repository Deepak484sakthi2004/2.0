// verify: debug error:E0119
use std::fmt::Display;

trait Render {
    fn render(&self) -> String;
}

// Blanket impl for every Display type...
impl<T: Display> Render for T {
    fn render(&self) -> String {
        self.to_string()
    }
}

// ...plus a specific impl for Vec<u8>. Vec<u8> is NOT Display today. Is this allowed?
impl Render for Vec<u8> {
    fn render(&self) -> String {
        format!("{} bytes", self.len())
    }
}

fn main() {
    println!("{}", vec![1u8, 2, 3].render());
}
