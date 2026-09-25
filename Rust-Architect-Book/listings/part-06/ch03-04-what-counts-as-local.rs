// verify: debug ok
use std::fmt;

trait Rule {
    fn id(&self) -> &str;
}

struct LargeAmount;

impl Rule for LargeAmount {
    fn id(&self) -> &str {
        "large-amount"
    }
}

// 1. `dyn LocalTrait` is a local type, so a foreign trait may be implemented for it.
impl fmt::Debug for dyn Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Rule({})", self.id())
    }
}

// 2. Box is #[fundamental]: Box<LocalType> counts as local too.
impl fmt::Display for Box<dyn Rule> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rule {}", self.id())
    }
}

// 3. A local type as a TRAIT PARAMETER makes `impl ForeignTrait<Local> for ForeignType` legal.
struct Cents(i64);

impl From<Cents> for f64 {
    fn from(c: Cents) -> f64 {
        c.0 as f64 / 100.0
    }
}

fn main() {
    let rules: Vec<Box<dyn Rule>> = vec![Box::new(LargeAmount)];
    println!("{rules:?}"); // Vec's Debug -> Box's Debug -> our `impl Debug for dyn Rule`
    println!("{}", rules[0]);
    let euros: f64 = Cents(12_550).into();
    println!("{euros}");
}
