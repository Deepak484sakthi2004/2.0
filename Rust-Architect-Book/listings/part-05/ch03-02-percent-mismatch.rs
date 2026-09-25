// verify: debug error:E0308
#[derive(Debug, Clone, Copy)]
pub struct Percent(u8);
#[derive(Debug, Clone, Copy)]
pub struct BasisPoints(u16);
#[derive(Debug, Clone, Copy)]
pub struct Cents(pub i64);

pub fn apply_discount(price: Cents, d: BasisPoints) -> Cents {
    Cents(price.0 - price.0 * d.0 as i64 / 10_000)
}

fn main() {
    let promo = Percent(15);
    // The 2.2 incident, replayed: a percent passed where basis points are expected.
    println!("{:?}", apply_discount(Cents(4_999), promo));
}
