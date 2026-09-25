// verify: debug error:E0034
trait Auditable {
    fn describe(&self) -> String;
}

trait Billable {
    fn describe(&self) -> String;
}

struct Invoice {
    id: u64,
}

impl Auditable for Invoice {
    fn describe(&self) -> String {
        format!("audit record for invoice {}", self.id)
    }
}

impl Billable for Invoice {
    fn describe(&self) -> String {
        format!("invoice #{} (billable)", self.id)
    }
}

fn main() {
    let inv = Invoice { id: 42 };
    println!("{}", inv.describe());
}
