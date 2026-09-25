// verify: debug ok
// verify: debug miri-ok
// verify: debug+tree miri-ok
//! What Pin's drop guarantee buys: a notifier whose waiters are nodes stored INSIDE the waiting
//! futures, linked into an intrusive list by address. No allocation per waiter. It's sound only
//! because (1) a pinned future never moves, so the address stays valid, and (2) its memory can't
//! be reused before its destructor runs, and the destructor unlinks the node.
//! tokio::sync::Notify and Semaphore keep their waiters this way [LIB].
use std::cell::Cell;
use std::future::Future;
use std::marker::PhantomPinned;
use std::pin::{pin, Pin};
use std::ptr;
use std::task::{Context, Poll, Waker};

struct Node {
    waker: Cell<Option<Waker>>,
    notified: Cell<bool>,
    linked: Cell<bool>,
    next: Cell<*const Node>,
}

struct Notify {
    head: Cell<*const Node>, // singly linked list of nodes that live inside pinned `Notified` futures
}

impl Notify {
    fn new() -> Notify {
        Notify { head: Cell::new(ptr::null()) }
    }

    fn notified(&self) -> Notified<'_> {
        let node = Node { waker: Cell::new(None), notified: Cell::new(false), linked: Cell::new(false), next: Cell::new(ptr::null()) };
        Notified { notify: self, node, _pin: PhantomPinned }
    }

    fn waiters(&self) -> usize {
        let (mut n, mut cur) = (0, self.head.get());
        while !cur.is_null() {
            n += 1;
            // SAFETY: a linked node lives in a pinned `Notified` that hasn't been dropped (Drop unlinks
            // it first), and pinned memory isn't moved or reused before drop. So `cur` is valid.
            cur = unsafe { &*cur }.next.get();
        }
        n
    }

    /// Wakes every waiter currently linked. Returns how many.
    fn notify_all(&self) -> usize {
        let mut woken = 0;
        let mut cur = self.head.replace(ptr::null());
        while !cur.is_null() {
            // SAFETY: as in `waiters`: every linked node is alive and hasn't moved.
            let node = unsafe { &*cur };
            cur = node.next.replace(ptr::null());
            node.linked.set(false);
            node.notified.set(true);
            if let Some(w) = node.waker.take() {
                w.wake();
            }
            woken += 1;
        }
        woken
    }
}

struct Notified<'a> {
    notify: &'a Notify,
    node: Node,
    _pin: PhantomPinned, // !Unpin: once polled, our address is in the list
}

impl Future for Notified<'_> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.into_ref().get_ref(); // shared access is enough: all state is in Cells
        if this.node.notified.get() {
            return Poll::Ready(());
        }
        this.node.waker.set(Some(cx.waker().clone())); // refresh on every poll (Chapter 12.2)
        if !this.node.linked.get() {
            this.node.linked.set(true);
            this.node.next.set(this.notify.head.get());
            this.notify.head.set(&this.node); // our address escapes: fine, we're pinned
        }
        Poll::Pending
    }
}

impl Drop for Notified<'_> {
    fn drop(&mut self) {
        // Treat `self` as pinned here: read and unlink, never move.
        if !self.node.linked.get() {
            return;
        }
        let me: *const Node = &self.node;
        let mut link = &self.notify.head;
        while !link.get().is_null() {
            if link.get() == me {
                link.set(self.node.next.get()); // unlink: nobody can reach this node any more
                return;
            }
            // SAFETY: nodes other than `me` are alive and linked (see `Notify::waiters`).
            link = unsafe { &(*link.get()).next };
        }
    }
}

fn poll_once(f: Pin<&mut Notified<'_>>) -> Poll<()> {
    f.poll(&mut Context::from_waker(Waker::noop()))
}

fn main() {
    let notify = Notify::new();
    let mut a = pin!(notify.notified());
    let mut b = pin!(notify.notified());
    {
        let mut c = pin!(notify.notified());
        println!("a, b, c polled: {:?} {:?} {:?}", poll_once(a.as_mut()), poll_once(b.as_mut()), poll_once(c.as_mut()));
        println!("linked waiters: {}", notify.waiters());
    } // c is dropped here (cancelled): its Drop unlinks the node before its stack slot dies
    println!("after dropping c: {}", notify.waiters());
    println!("notify_all woke {}", notify.notify_all());
    println!("a, b polled again: {:?} {:?}", poll_once(a.as_mut()), poll_once(b.as_mut()));
    println!("size_of::<Notified>() = {} bytes (the node is a field of the future: no Box, no Vec)", size_of::<Notified<'_>>());
}
