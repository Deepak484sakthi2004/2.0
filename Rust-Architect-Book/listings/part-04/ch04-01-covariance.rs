// verify: debug ok
fn pick<'a>(primary: &'a str, fallback: &'a str, use_primary: bool) -> &'a str {
    if use_primary { primary } else { fallback }
}

fn main() {
    let static_default: &'static str = "eu-west-1"; // lives for the whole program
    let from_request = String::from("ap-south-1"); // lives until the end of main
    // &'static str is a SUBTYPE of &'a str: it may be used wherever a shorter-lived one is expected.
    let region = pick(&from_request, static_default, false);
    println!("region = {region}");

    // Covariance lifts that through containers: Vec<&'static str> can become Vec<&'a str>.
    let defaults: Vec<&'static str> = vec!["us-east-1", "eu-west-1"];
    let mut candidates: Vec<&str> = defaults; // moved in, with its lifetime shortened
    candidates.push(&from_request);
    println!("candidates = {candidates:?}");
}
