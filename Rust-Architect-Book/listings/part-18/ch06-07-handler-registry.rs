// verify: debug ok
// verify: release ok
// A webhook-handler registry that de-duplicates by function address. Two DIFFERENT handlers with
// identical bodies are merged by LLVM in release builds, so the second registration is dropped.
// (Written with fn_addr_eq, as the unpredictable_function_pointer_comparisons lint suggests, which
// silences the warning without fixing the logic.)
#[derive(Debug, Clone, Copy, PartialEq)]
enum Outcome {
    Ack,
}

struct Event {
    kind: &'static str,
}

// Placeholders until the v2 flows ship: both just acknowledge.
fn ack_refund(_e: &Event) -> Outcome {
    Outcome::Ack
}
fn ack_chargeback(_e: &Event) -> Outcome {
    Outcome::Ack
}

type Handler = fn(&Event) -> Outcome;

struct Registry {
    handlers: Vec<(&'static str, Handler)>,
}

impl Registry {
    fn register(&mut self, kind: &'static str, h: Handler) {
        // BUG: "don't register the same handler twice", keyed on the function's address.
        if self.handlers.iter().any(|&(_, existing)| std::ptr::fn_addr_eq(existing, h)) {
            return;
        }
        self.handlers.push((kind, h));
    }

    fn dispatch(&self, e: &Event) -> Option<Outcome> {
        self.handlers.iter().find(|(k, _)| *k == e.kind).map(|(_, h)| h(e))
    }
}

fn main() {
    let mut r = Registry { handlers: Vec::new() };
    r.register("refund", ack_refund);
    r.register("chargeback", ack_chargeback);
    let kinds: Vec<&str> = r.handlers.iter().map(|(k, _)| *k).collect();
    println!("registered: {kinds:?}");
    println!("chargeback -> {:?}", r.dispatch(&Event { kind: "chargeback" }));
}
