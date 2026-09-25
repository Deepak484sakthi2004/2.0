// verify: debug error:E0597
fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() >= b.len() { a } else { b }
}

fn main() {
    let service = String::from("gateway");
    let winner;
    {
        let region = String::from("eu-west-1");
        winner = longest(&service, &region); // 'a must cover BOTH inputs' loans
    } // `region` dropped here
    println!("{winner}");
}
