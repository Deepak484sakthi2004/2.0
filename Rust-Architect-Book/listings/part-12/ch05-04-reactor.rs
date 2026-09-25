// verify: release ok
//! A reactor: when no task is runnable, the executor blocks in epoll_wait (through mio), and
//! readiness events wake the tasks waiting for them. An echo server and 100 clients run as
//! tasks on ONE thread. Wakers carry only a task id, so futures themselves can be !Send.
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Token};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::{poll_fn, Future};
use std::io::{self, ErrorKind, Read, Write};
use std::net::SocketAddr;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

// ---------- run queue and wakers ----------

#[derive(Default)]
struct RunQueue {
    ids: VecDeque<usize>,
    queued: HashSet<usize>,
}

struct TaskWaker {
    id: usize,
    queue: Arc<Mutex<RunQueue>>,
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        let mut q = self.queue.lock().unwrap();
        if q.queued.insert(self.id) {
            q.ids.push_back(self.id);
        }
    }
}

// ---------- the reactor: mio registrations and the wakers waiting on them ----------

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Dir {
    Read,
    Write,
}

struct Reactor {
    poll: RefCell<mio::Poll>,
    wakers: RefCell<HashMap<(Token, Dir), Waker>>,
    next_token: Cell<usize>,
    waits: Cell<u32>,
}

impl Reactor {
    fn register(&self, source: &mut impl mio::event::Source) -> Token {
        let token = Token(self.next_token.get());
        self.next_token.set(token.0 + 1);
        self.poll.borrow().registry().register(source, token, Interest::READABLE | Interest::WRITABLE).unwrap();
        token
    }

    fn wait_for(&self, token: Token, dir: Dir, cx: &Context<'_>) {
        self.wakers.borrow_mut().insert((token, dir), cx.waker().clone());
    }

    /// Block until at least one registered source is ready, then wake whoever waits on it.
    fn turn(&self) -> bool {
        let mut events = Events::with_capacity(256);
        self.poll.borrow_mut().poll(&mut events, Some(Duration::from_secs(2))).unwrap();
        self.waits.set(self.waits.get() + 1);
        let mut wakers = self.wakers.borrow_mut();
        for ev in events.iter() {
            if ev.is_readable() || ev.is_read_closed() || ev.is_error() {
                if let Some(w) = wakers.remove(&(ev.token(), Dir::Read)) {
                    w.wake();
                }
            }
            if ev.is_writable() || ev.is_write_closed() || ev.is_error() {
                if let Some(w) = wakers.remove(&(ev.token(), Dir::Write)) {
                    w.wake();
                }
            }
        }
        !events.is_empty()
    }
}

// ---------- the executor ----------

type LocalFuture = Pin<Box<dyn Future<Output = ()>>>;

struct Runtime {
    tasks: RefCell<slab::Slab<Option<LocalFuture>>>,
    queue: Arc<Mutex<RunQueue>>,
    reactor: Reactor,
    polls: Cell<u32>,
}

impl Runtime {
    fn new() -> Rc<Runtime> {
        Rc::new(Runtime {
            tasks: RefCell::default(),
            queue: Arc::default(),
            reactor: Reactor {
                poll: RefCell::new(mio::Poll::new().unwrap()),
                wakers: RefCell::default(),
                next_token: Cell::new(0),
                waits: Cell::new(0),
            },
            polls: Cell::new(0),
        })
    }

    fn spawn(&self, fut: impl Future<Output = ()> + 'static) {
        let id = self.tasks.borrow_mut().insert(Some(Box::pin(fut)));
        let mut q = self.queue.lock().unwrap();
        q.queued.insert(id);
        q.ids.push_back(id);
    }

    fn run(&self) {
        loop {
            // 1. Poll everything that's runnable.
            loop {
                let next = {
                    let mut q = self.queue.lock().unwrap();
                    let id = q.ids.pop_front();
                    if let Some(id) = id {
                        q.queued.remove(&id);
                    }
                    id
                };
                let Some(id) = next else { break };
                // Take the future out, so a task can spawn others while it's being polled.
                let Some(mut fut) = self.tasks.borrow_mut().get_mut(id).and_then(Option::take) else { continue };
                let waker = Waker::from(Arc::new(TaskWaker { id, queue: self.queue.clone() }));
                self.polls.set(self.polls.get() + 1);
                if fut.as_mut().poll(&mut Context::from_waker(&waker)).is_pending() {
                    self.tasks.borrow_mut()[id] = Some(fut);
                } else {
                    self.tasks.borrow_mut().remove(id);
                }
            }
            if self.tasks.borrow().is_empty() {
                return;
            }
            // 2. Nothing runnable: park in epoll_wait until I/O makes something runnable.
            if !self.reactor.turn() {
                println!("stuck: {} tasks waiting, no I/O for 2 s", self.tasks.borrow().len());
                return;
            }
        }
    }
}

// ---------- async TCP on top of the reactor ----------

struct AsyncListener {
    inner: TcpListener,
    token: Token,
    rt: Rc<Runtime>,
}

impl AsyncListener {
    fn bind(rt: &Rc<Runtime>, addr: &str) -> AsyncListener {
        let mut inner = TcpListener::bind(addr.parse().unwrap()).unwrap();
        let token = rt.reactor.register(&mut inner);
        AsyncListener { inner, token, rt: rt.clone() }
    }

    async fn accept(&self) -> io::Result<AsyncStream> {
        let stream = poll_fn(|cx| match self.inner.accept() {
            Ok((s, _)) => Poll::Ready(Ok(s)),
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                self.rt.reactor.wait_for(self.token, Dir::Read, cx);
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        })
        .await?;
        Ok(AsyncStream::new(&self.rt, stream))
    }
}

struct AsyncStream {
    inner: TcpStream,
    token: Token,
    rt: Rc<Runtime>,
}

impl AsyncStream {
    fn new(rt: &Rc<Runtime>, mut inner: TcpStream) -> AsyncStream {
        let token = rt.reactor.register(&mut inner);
        AsyncStream { inner, token, rt: rt.clone() }
    }

    async fn connect(rt: &Rc<Runtime>, addr: SocketAddr) -> io::Result<AsyncStream> {
        let s = AsyncStream::new(rt, TcpStream::connect(addr)?); // nonblocking: may still be in progress
        poll_fn(|cx| {
            if let Some(e) = s.inner.take_error()? {
                return Poll::Ready(Err(e));
            }
            match s.inner.peer_addr() {
                Ok(_) => Poll::Ready(Ok(())),
                Err(e) if e.kind() == ErrorKind::NotConnected => {
                    s.rt.reactor.wait_for(s.token, Dir::Write, cx);
                    Poll::Pending
                }
                Err(e) => Poll::Ready(Err(e)),
            }
        })
        .await?;
        Ok(s)
    }

    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        poll_fn(|cx| match self.inner.read(buf) {
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                self.rt.reactor.wait_for(self.token, Dir::Read, cx);
                Poll::Pending
            }
            other => Poll::Ready(other),
        })
        .await
    }

    async fn write_all(&mut self, mut data: &[u8]) -> io::Result<()> {
        while !data.is_empty() {
            let n = poll_fn(|cx| match self.inner.write(data) {
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    self.rt.reactor.wait_for(self.token, Dir::Write, cx);
                    Poll::Pending
                }
                other => Poll::Ready(other),
            })
            .await?;
            data = &data[n..];
        }
        Ok(())
    }
}

impl Drop for AsyncStream {
    fn drop(&mut self) {
        let mut wakers = self.rt.reactor.wakers.borrow_mut();
        wakers.remove(&(self.token, Dir::Read));
        wakers.remove(&(self.token, Dir::Write));
    }
}

// ---------- demo: echo server + clients, one thread ----------

fn main() {
    const CLIENTS: usize = 100;
    let rt = Runtime::new();
    let listener = AsyncListener::bind(&rt, "127.0.0.1:0");
    let addr = listener.inner.local_addr().unwrap();

    let server_rt = rt.clone();
    rt.spawn(async move {
        for _ in 0..CLIENTS {
            let mut conn = listener.accept().await.unwrap();
            server_rt.spawn(async move {
                let mut buf = [0u8; 64];
                let n = conn.read(&mut buf).await.unwrap();
                conn.write_all(&buf[..n]).await.unwrap();
            });
        }
    });

    let echoed = Rc::new(Cell::new(0));
    for i in 0..CLIENTS {
        let (rt2, echoed) = (rt.clone(), echoed.clone());
        rt.spawn(async move {
            let mut s = AsyncStream::connect(&rt2, addr).await.unwrap();
            let msg = format!("hello {i}");
            s.write_all(msg.as_bytes()).await.unwrap();
            let mut buf = [0u8; 64];
            let n = s.read(&mut buf).await.unwrap(); // one small message: one read on loopback
            if &buf[..n] == msg.as_bytes() {
                echoed.set(echoed.get() + 1);
            }
        });
    }

    rt.run();
    let threads = std::fs::read_to_string("/proc/self/status").unwrap();
    let threads = threads.lines().find(|l| l.starts_with("Threads:")).unwrap().split_whitespace().nth(1).unwrap().to_string();
    println!("{} of {CLIENTS} clients echoed", echoed.get());
    println!("threads: {threads}; tasks spawned: {}; polls: {}; epoll_wait calls: {}", 1 + 2 * CLIENTS, rt.polls.get(), rt.reactor.waits.get());
}
