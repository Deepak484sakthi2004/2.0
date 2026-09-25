// verify: debug ok
mod pricing {
    /// A discount in whole percent, 0..=100. Private field: only `Percent::new` creates one.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Percent(u8);

    /// A discount in basis points (1/100 of a percent), 0..=10_000.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct BasisPoints(u16);

    /// Money in minor units (cents). Signed: refunds are negative.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Cents(pub i64);

    impl Percent {
        pub fn new(p: u8) -> Option<Percent> {
            (p <= 100).then_some(Percent(p))
        }
    }

    impl BasisPoints {
        pub fn new(bps: u16) -> Option<BasisPoints> {
            (bps <= 10_000).then_some(BasisPoints(bps))
        }
    }

    /// The one sanctioned conversion: lossless, so it is `From`, not a cast at the call site.
    impl From<Percent> for BasisPoints {
        fn from(p: Percent) -> BasisPoints {
            BasisPoints(p.0 as u16 * 100)
        }
    }

    /// The only discount function. It takes BasisPoints; a Percent must be converted explicitly.
    pub fn apply_discount(price: Cents, d: BasisPoints) -> Cents {
        // d <= 10_000 by construction, so the result is in 0..=price: no underflow possible.
        let off = price.0 as i128 * d.0 as i128 / 10_000;
        Cents(price.0 - off as i64)
    }
}

use pricing::{apply_discount, BasisPoints, Cents, Percent};

fn main() {
    let price = Cents(4_999);
    let promo = Percent::new(15).unwrap(); // marketing speaks percent
    let partner = BasisPoints::new(250).unwrap(); // the partner API speaks basis points

    println!("15%     -> {:?}", apply_discount(price, promo.into()));
    println!("250 bps -> {:?}", apply_discount(price, partner));
    println!("Percent::new(150) = {:?}", Percent::new(150));
    println!("BasisPoints::new(15_000) = {:?}", BasisPoints::new(15_000));
    println!("size_of::<BasisPoints>() = {}", std::mem::size_of::<BasisPoints>());
}
