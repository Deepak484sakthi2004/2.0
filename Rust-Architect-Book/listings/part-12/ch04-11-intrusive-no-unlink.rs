// verify: debug miri dangling
//! ch04-10 with ONE thing removed: the Drop impl that unlinks a cancelled waiter. Nothing else
//! changes, and it still compiles. When `c` is dropped, its node stays in the list; its stack slot
//! dies; `notify_all` then follows the stale pointer. Miri reports the use-after-free. Not run
//! natively: it is undefined behavior.
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
            // SAFETY (claimed; FALSE in this file, which has no Drop impl): a linked node lives in a
            // pinned `Notified` that hasn't been dropped (Drop unlinks it first), and pinned memory isn't
            // moved or reused before drop. So `cur` is valid.
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
    } // c is dropped here (cancelled). BUG: nothing unlinks its node, and its stack slot is now dead
    println!("after dropping c: {}", notify.waiters());
    println!("notify_all woke {}", notify.notify_all());
}
