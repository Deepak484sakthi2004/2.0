// verify: debug ok
//! Cancellation is drop. Dropping a suspended future drops the locals that are live at its current
//! .await, and nothing after that await ever runs. Chapter 12.3's failure scenario.
use futures::future::{self, Either};
use std::cell::RefCell;
use std::future::Future;
use std::time::Duration;

fn sleep_ms(ms: u64) -> impl Future<Output = ()> {
    let (tx, rx) = futures::channel::oneshot::channel::<()>();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(ms));
        let _ = tx.send(());
    });
    async move {
        let _ = rx.await;
    }
}

#[derive(Debug, Default)]
struct Escrow {
    available: i64,
    reserved: i64,
    paid_out: i64,
}

/// v1: reserve, await the fraud check, then pay out. A cancellation between the two steps
/// leaves the money reserved forever.
async fn payout_v1(escrow: &RefCell<Escrow>, amount: i64) {
    {
        let mut e = escrow.borrow_mut();
        e.available -= amount;
        e.reserved += amount;
    }
    sleep_ms(50).await; // fraud check (slow today)
    let mut e = escrow.borrow_mut();
    e.reserved -= amount;
    e.paid_out += amount;
}

/// v2: the reservation is a guard. If the future is dropped before `commit`, Drop releases it.
struct Reservation<'a> {
    escrow: &'a RefCell<Escrow>,
    amount: i64,
    committed: bool,
}

impl<'a> Reservation<'a> {
    fn new(escrow: &'a RefCell<Escrow>, amount: i64) -> Self {
        let mut e = escrow.borrow_mut();
        e.available -= amount;
        e.reserved += amount;
        Reservation { escrow, amount, committed: false }
    }
    fn commit(mut self) {
        let mut e = self.escrow.borrow_mut();
        e.reserved -= self.amount;
        e.paid_out += self.amount;
        self.committed = true;
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if !self.committed {
            let mut e = self.escrow.borrow_mut();
            e.reserved -= self.amount;
            e.available += self.amount;
            println!("   reservation of {} released by Drop (future cancelled)", self.amount);
        }
    }
}

async fn payout_v2(escrow: &RefCell<Escrow>, amount: i64) {
    let reservation = Reservation::new(escrow, amount); // live across the await: dropped on cancel
    sleep_ms(50).await;
    reservation.commit();
}

/// Run `fut` with a 10 ms deadline, the way a request timeout would.
async fn with_timeout<F: Future>(fut: F) -> Option<F::Output> {
    match future::select(Box::pin(fut), Box::pin(sleep_ms(10))).await {
        Either::Left((out, _)) => Some(out),
        Either::Right(((), _unfinished)) => None, // `_unfinished` is dropped at the end of this arm
    }
}

fn main() {
    let escrow = RefCell::new(Escrow { available: 1_000, ..Default::default() });
    let r = futures::executor::block_on(with_timeout(payout_v1(&escrow, 300)));
    println!("v1 timed out: {}; escrow {:?}", r.is_none(), escrow.borrow());

    let escrow = RefCell::new(Escrow { available: 1_000, ..Default::default() });
    let r = futures::executor::block_on(with_timeout(payout_v2(&escrow, 300)));
    println!("v2 timed out: {}; escrow {:?}", r.is_none(), escrow.borrow());
}
