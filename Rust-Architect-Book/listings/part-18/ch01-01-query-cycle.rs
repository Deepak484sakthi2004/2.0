// verify: debug error:E0391
// Two constants defined in terms of each other. Each one's value is a query result that needs
// the other's, so the query system detects a cycle and names the queries involved.
const LIMIT: usize = BURST * 2;
const BURST: usize = LIMIT / 2;

fn main() {
    println!("{LIMIT}");
}
