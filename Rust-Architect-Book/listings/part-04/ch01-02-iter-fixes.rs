// verify: debug ok
fn main() {
    // Fix 1: collect first (the loan on `orders` ends), then mutate.
    let mut orders = vec![1, 2, 3];
    let follow_ups: Vec<i32> = orders.iter().filter(|&&id| id == 2).map(|_| 4).collect();
    orders.extend(follow_ups);
    println!("collect-then-extend: {orders:?}");

    // Fix 2: a purpose-built method that owns the whole iteration: retain.
    let mut orders = vec![1, 2, 3, 4, 5, 6];
    orders.retain(|id| id % 2 == 0);
    println!("retain evens:        {orders:?}");

    // Fix 3: iterate by index over a length fixed up front, mutating through the Vec itself.
    let mut orders = vec![1, 2, 3];
    let n = orders.len();
    for i in 0..n {
        if orders[i] == 2 {
            orders.push(4);
        }
    }
    println!("index loop:          {orders:?}");
}
