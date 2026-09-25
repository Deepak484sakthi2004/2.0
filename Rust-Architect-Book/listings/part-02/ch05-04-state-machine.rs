// verify: debug ok
#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Pending,
    Authorized,
    Captured,
    Refunded,
    Failed,
}

#[derive(Debug, Clone, Copy)]
enum Event {
    Authorize,
    Capture,
    Refund,
    Fail,
}

fn next(state: State, event: Event) -> Result<State, String> {
    use Event::*;
    use State::*;
    match (state, event) {
        (Pending, Authorize) => Ok(Authorized),
        (Pending | Authorized, Fail) => Ok(Failed),
        (Authorized, Capture) => Ok(Captured),
        (Captured, Refund) => Ok(Refunded),
        (s @ (Refunded | Failed), e) => Err(format!("{s:?} is terminal; rejected {e:?}")),
        (s, e) => Err(format!("invalid transition {s:?} + {e:?}")),
    }
}

fn main() {
    let mut state = State::Pending;
    for event in [Event::Authorize, Event::Capture, Event::Refund, Event::Capture] {
        match next(state, event) {
            Ok(s) => {
                println!("{state:?} --{event:?}--> {s:?}");
                state = s;
            }
            Err(e) => println!("rejected: {e}"),
        }
    }
    println!("{:?}", next(State::Pending, Event::Refund));
    println!("{:?}", next(State::Authorized, Event::Fail));
}
