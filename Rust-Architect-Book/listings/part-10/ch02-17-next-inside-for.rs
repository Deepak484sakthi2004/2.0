// verify: debug error:E0499
// Skip the line after every "#continued" marker, by calling next() inside the loop.
fn main() {
    let input = "a\n#continued\nskip-me\nb";
    let mut lines = input.lines();
    for line in lines.by_ref() {
        if line == "#continued" {
            lines.next(); // the for loop holds `lines` mutably borrowed for its whole duration
            continue;
        }
        println!("{line}");
    }
}
