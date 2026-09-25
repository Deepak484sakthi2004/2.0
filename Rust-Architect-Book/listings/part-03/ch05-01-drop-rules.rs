// verify: debug ok
struct Noisy(&'static str);

impl Drop for Noisy {
    fn drop(&mut self) {
        println!("  drop {}", self.0);
    }
}

fn make(name: &'static str) -> Noisy {
    Noisy(name)
}

fn main() {
    println!("1. a temporary dies at the end of its statement:");
    let len = make("temp").0.len();
    println!("  len = {len}");

    println!("2. assigning over a value drops the old one:");
    let mut slot = make("old");
    slot = make("new");
    println!("  slot now holds {}", slot.0);

    println!("3. explicit drop(): just a move into a function that does nothing:");
    let early = make("early");
    drop(early);
    println!("  after drop(early)");

    println!("4. tuple fields and Vec elements drop front to back; locals in reverse:");
    {
        let _pair = (make("pair.0"), make("pair.1"));
        let _v = vec![make("v[0]"), make("v[1]")];
    }

    println!("5. mem::forget: never dropped (safe; it leaks):");
    std::mem::forget(make("forgotten"));

    println!("6. end of main: remaining locals in reverse declaration order:");
    let _last = make("last");
}
