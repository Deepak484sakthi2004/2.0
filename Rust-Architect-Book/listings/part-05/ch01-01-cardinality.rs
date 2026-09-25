// verify: debug ok
// Counting states: a struct of two bools has 4 states; the domain has only 3.
#[derive(Debug, Clone, Copy)]
struct ConnFlags {
    connected: bool,
    authenticated: bool,
}

#[derive(Debug, Clone, Copy)]
enum ConnState {
    Disconnected,
    Connected,
    Authenticated,
}

fn flags_valid(f: ConnFlags) -> bool {
    // the invariant every function touching ConnFlags must remember
    !(f.authenticated && !f.connected)
}

fn main() {
    let all_flags = [
        ConnFlags { connected: false, authenticated: false },
        ConnFlags { connected: false, authenticated: true },
        ConnFlags { connected: true, authenticated: false },
        ConnFlags { connected: true, authenticated: true },
    ];
    for f in all_flags {
        println!("{f:?} -> valid: {}", flags_valid(f));
    }
    let all_states = [ConnState::Disconnected, ConnState::Connected, ConnState::Authenticated];
    println!("ConnFlags: {} representable states; ConnState: {}", all_flags.len(), all_states.len());
    for s in all_states {
        println!("{s:?} -> valid by construction");
    }
}
