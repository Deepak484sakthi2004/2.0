// verify: debug ok
// verify: release ok
// The fix: identity is the thing you mean (the event kind), never a function's address.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Outcome {
    Ack,
}

struct Event {
    kind: &'static str,
}

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
    fn register(&mut self, kind: &'static str, h: Handler) -> Result<(), String> {
        if self.handlers.iter().any(|(k, _)| *k == kind) {
            return Err(format!("handler for {kind:?} already registered"));
        }
        self.handlers.push((kind, h));
        Ok(())
    }

    fn dispatch(&self, e: &Event) -> Option<Outcome> {
        self.handlers.iter().find(|(k, _)| *k == e.kind).map(|(_, h)| h(e))
    }
}

fn main() {
    let mut r = Registry { handlers: Vec::new() };
    r.register("refund", ack_refund).unwrap();
    r.register("chargeback", ack_chargeback).unwrap();
    println!("{:?}", r.register("refund", ack_refund));
    let kinds: Vec<&str> = r.handlers.iter().map(|(k, _)| *k).collect();
    println!("registered: {kinds:?}");
    println!("chargeback -> {:?}", r.dispatch(&Event { kind: "chargeback" }));
}
