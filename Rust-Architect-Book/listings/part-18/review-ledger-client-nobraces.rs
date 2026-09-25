// verify: debug error:E0499
// The capstone's main() with the braces around the journal removed. The Journal (which holds
// `&mut out`) now lives to the end of main, and its Drop is a use of that loan.
pub trait Sink {
    fn write(&mut self, line: &str);
}

pub struct Stdout;

impl Sink for Stdout {
    fn write(&mut self, line: &str) {
        println!("  sink: {line}");
    }
}

pub struct Journal<'a> {
    sink: &'a mut dyn Sink,
    entries: u32,
}

impl Drop for Journal<'_> {
    fn drop(&mut self) {
        let line = format!("journal closed after {} entries", self.entries);
        self.sink.write(&line);
    }
}

fn main() {
    let mut out = Stdout;
    let mut j = Journal { sink: &mut out, entries: 0 };
    j.entries += 1;
    out.write("done");
}
