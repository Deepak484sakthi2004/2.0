// verify: debug error:E0210
struct Cents(i64);
impl<T> From<Cents> for T {
    fn from(c: Cents) -> T { unimplemented!("{}", c.0) }
}
fn main() {}
