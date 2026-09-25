// verify: debug ok
// Answer-key check (Chapter 10.2 debugging exercise): two fixes for calling next() inside a for loop.
fn with_while_let(input: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut lines = input.lines();
    while let Some(line) = lines.next() {
        if line == "#continued" {
            lines.next(); // a separate, short borrow: the previous next() call has already returned
            continue;
        }
        out.push(line);
    }
    out
}

fn with_flag(input: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut skip = false;
    for line in input.lines() {
        if std::mem::take(&mut skip) {
            continue;
        }
        if line == "#continued" {
            skip = true;
            continue;
        }
        out.push(line);
    }
    out
}

fn main() {
    let input = "a\n#continued\nskip-me\nb";
    println!("{:?} {:?}", with_while_let(input), with_flag(input));
}
