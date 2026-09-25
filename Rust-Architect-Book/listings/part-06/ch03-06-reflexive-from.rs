// verify: debug error:E0119
struct Cents(i64);
impl<T> From<T> for Cents {
    fn from(_t: T) -> Cents { Cents(0) }
}
fn main() { let c = Cents(1); println!("{}", c.0); }
