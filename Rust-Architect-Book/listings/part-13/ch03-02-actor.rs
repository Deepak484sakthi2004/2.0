// verify: debug ok
//! The actor pattern: one task owns the state; everyone else sends it messages with a oneshot for the reply.
//! No lock exists, the mailbox bound is the backpressure, and a dead actor is visible to every caller.
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};

enum Command {
    Reserve { merchant: u32, cents: i64, reply: oneshot::Sender<Result<i64, String>> },
    Balance { merchant: u32, reply: oneshot::Sender<i64> },
}

/// The only owner of `limits`: runs until every handle is dropped.
async fn limits_actor(mut inbox: mpsc::Receiver<Command>, mut limits: HashMap<u32, i64>) {
    while let Some(cmd) = inbox.recv().await {
        match cmd {
            Command::Reserve { merchant, cents, reply } => {
                assert!(cents >= 0, "negative reservation reached the actor"); // a bug: kills the actor
                let left = limits.entry(merchant).or_insert(0);
                let result = if *left >= cents {
                    *left -= cents;
                    Ok(*left)
                } else {
                    Err(format!("limit exceeded: {left} left"))
                };
                let _ = reply.send(result); // the caller may have given up: that's fine
            }
            Command::Balance { merchant, reply } => {
                let _ = reply.send(*limits.get(&merchant).unwrap_or(&0));
            }
        }
    }
}

#[derive(Clone)]
struct LimitsHandle(mpsc::Sender<Command>);

impl LimitsHandle {
    async fn reserve(&self, merchant: u32, cents: i64) -> Result<i64, String> {
        let (reply, rx) = oneshot::channel();
        self.0.send(Command::Reserve { merchant, cents, reply }).await.map_err(|_| "limits actor is gone".to_string())?;
        rx.await.map_err(|_| "limits actor died before replying".to_string())?
    }
    async fn balance(&self, merchant: u32) -> Result<i64, String> {
        let (reply, rx) = oneshot::channel();
        self.0.send(Command::Balance { merchant, reply }).await.map_err(|_| "limits actor is gone".to_string())?;
        rx.await.map_err(|_| "limits actor died before replying".to_string())
    }
}

#[tokio::main]
async fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let (tx, rx) = mpsc::channel(64); // the mailbox bound: at most 64 requests wait
    let actor = tokio::spawn(limits_actor(rx, HashMap::from([(7, 10_000)])));
    let limits = LimitsHandle(tx);

    // 100 concurrent reservations of 150 cents against a 10,000-cent limit: exactly 66 can succeed.
    let calls: Vec<_> = (0..100)
        .map(|_| {
            let limits = limits.clone();
            tokio::spawn(async move { limits.reserve(7, 150).await.is_ok() })
        })
        .collect();
    let mut ok = 0;
    for c in calls {
        ok += c.await.unwrap() as u32;
    }
    println!("reservations granted: {ok} of 100; balance now {:?}", limits.balance(7).await);

    // A bug in a message handler kills the actor. Callers see errors, not hangs.
    println!("negative reservation: {:?}", limits.reserve(7, -5).await);
    println!("next call:            {:?}", limits.balance(7).await);
    println!("actor task: panicked = {}", actor.await.unwrap_err().is_panic());
}
