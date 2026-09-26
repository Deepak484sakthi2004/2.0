// verify: debug ok
// verify: debug test
// verify: release ok
//! Ferrite v2: Ferrite v1's store and line protocol, served by Tokio.
//!
//! - store:    v1's `KvStore` trait (the same three required methods, plus one defaulted method, `get_shared`)
//!             and `ShardedStore`, which now stores values as `Arc<[u8]>`: a GET clones a pointer under the shard
//!             lock instead of copying the value (Chapter 20.7, listing part-20/ch07-05: 1.9-2.8x faster).
//! - protocol: v1's grammar and replies. Errors now carry a stable code: `-ERR <CODE> <detail>`.
//! - codec:    a tokio-util Decoder that finds request lines in whatever bytes have arrived (a request may be
//!             split across reads, or several may arrive in one), bounded by max_line; an Encoder for replies.
//! - server:   one task per connection, a connection limit (Semaphore, refuse with BUSY), idle and write
//!             timeouts, a cap on unflushed reply bytes, graceful shutdown with a drain deadline, per-server stats.

/// v1's store (listing part-11/project-04-ferrite-v1.rs), with values stored as `Arc<[u8]>`.
mod store {
    use std::collections::HashMap;
    use std::hash::{BuildHasher, RandomState};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

    /// The Ferrite storage contract. Every method takes `&self`: implementations synchronize internally,
    /// so one store can be shared by every connection (`Arc<S>` where `S: KvStore + Send + Sync`).
    /// v1: in memory. v3 (Part XXIII): WAL + LSM on disk, behind this same trait.
    pub trait KvStore {
        /// A copy of the value, if the key exists.
        fn get(&self, key: &[u8]) -> Option<Vec<u8>>;
        /// Inserts or replaces; returns the previous value.
        fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Option<Vec<u8>>;
        /// Removes; returns the removed value.
        fn delete(&self, key: &[u8]) -> Option<Vec<u8>>;
        /// v2: the value as a shared, immutable buffer. Defaulted, so every v1 implementation still compiles;
        /// stores that keep `Arc<[u8]>` values override it to avoid the copy.
        fn get_shared(&self, key: &[u8]) -> Option<Arc<[u8]>> {
            self.get(key).map(Arc::from)
        }
    }

    type Map = HashMap<Vec<u8>, Arc<[u8]>>;

    pub struct ShardedStore {
        shards: Vec<RwLock<Map>>,
        router: RandomState, // keyed SipHash: clients can't aim many keys at one shard (Chapter 9.3)
        poison_recoveries: AtomicU64,
    }

    impl ShardedStore {
        pub fn new(shards: usize) -> ShardedStore {
            assert!(shards > 0, "at least one shard");
            ShardedStore {
                shards: (0..shards).map(|_| RwLock::new(HashMap::new())).collect(),
                router: RandomState::new(),
                poison_recoveries: AtomicU64::new(0),
            }
        }

        fn shard(&self, key: &[u8]) -> &RwLock<Map> {
            let h = self.router.hash_one(key);
            &self.shards[(h % self.shards.len() as u64) as usize]
        }

        // Poisoning policy: RECOVER (and count). Under a shard lock we run nothing but HashMap get/insert/
        // remove on Vec<u8> keys, whose Hash and Eq cannot panic, and entries have no cross-key invariant.
        // A map observed after some panic is still a valid map. (Contrast Chapter 11.3's ledger, whose
        // invariant spans two fields: there, recovering blindly would be wrong.)
        fn read<'a>(&'a self, shard: &'a RwLock<Map>) -> RwLockReadGuard<'a, Map> {
            shard.read().unwrap_or_else(|poisoned| {
                self.note_recovery(shard);
                poisoned.into_inner()
            })
        }

        fn write<'a>(&'a self, shard: &'a RwLock<Map>) -> RwLockWriteGuard<'a, Map> {
            shard.write().unwrap_or_else(|poisoned| {
                self.note_recovery(shard);
                poisoned.into_inner()
            })
        }

        fn note_recovery(&self, shard: &RwLock<Map>) {
            self.poison_recoveries.fetch_add(1, Ordering::Relaxed);
            shard.clear_poison(); // recover once, not on every later access
        }

        /// Sum of the shard sizes. Not a snapshot: other threads may change shards while we count.
        pub fn len(&self) -> usize {
            self.shards.iter().map(|s| self.read(s).len()).sum()
        }

        pub fn shard_lens(&self) -> Vec<usize> {
            self.shards.iter().map(|s| self.read(s).len()).collect()
        }

        pub fn poison_recoveries(&self) -> u64 {
            self.poison_recoveries.load(Ordering::Relaxed)
        }

        /// Test hook: panic while holding the write lock of `key`'s shard, poisoning it.
        #[cfg(test)]
        pub fn poison_shard_of(&self, key: &[u8]) {
            let shard = self.shard(key);
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _guard = shard.write().unwrap();
                panic!("simulated bug while holding a shard lock");
            }));
        }
    }

    impl KvStore for ShardedStore {
        fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
            self.get_shared(key).map(|v| v.to_vec()) // the copy happens after the lock is released
        }

        fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Option<Vec<u8>> {
            let value: Arc<[u8]> = Arc::from(value); // allocate and copy before taking the lock
            let shard = self.shard(&key);
            let previous = self.write(shard).insert(key, value);
            previous.map(|v| v.to_vec()) // after the guard is gone
        }

        fn delete(&self, key: &[u8]) -> Option<Vec<u8>> {
            let removed = self.write(self.shard(key)).remove(key);
            removed.map(|v| v.to_vec())
        }

        fn get_shared(&self, key: &[u8]) -> Option<Arc<[u8]>> {
            self.read(self.shard(key)).get(key).cloned() // under the lock: a reference-count increment
        }
    }
}

/// v1's grammar and replies; errors gain a stable code.
mod protocol {
    use crate::store::KvStore;
    use std::sync::Arc;
    use bytes::{BufMut, BytesMut};

    #[derive(Debug, PartialEq)]
    pub enum Request {
        Ping,
        Get(Vec<u8>),
        Set(Vec<u8>, Vec<u8>),
        Del(Vec<u8>),
    }

    /// v2: every error reply starts with a code. Clients switch on the code; the detail text after it is for
    /// people and may change between releases (Chapter 8.2: never match on messages).
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum Code {
        Busy,
        ShuttingDown,
        LineTooLong,
        InvalidUtf8,
        Empty,
        UnknownCommand,
        WrongArity,
    }

    impl Code {
        pub fn as_str(self) -> &'static str {
            match self {
                Code::Busy => "BUSY",
                Code::ShuttingDown => "SHUTTING_DOWN",
                Code::LineTooLong => "LINE_TOO_LONG",
                Code::InvalidUtf8 => "INVALID_UTF8",
                Code::Empty => "EMPTY",
                Code::UnknownCommand => "UNKNOWN_COMMAND",
                Code::WrongArity => "WRONG_ARITY",
            }
        }
    }

    #[derive(Debug, PartialEq)]
    pub enum Response {
        Pong,
        Ok,
        Value(Arc<[u8]>), // shared with the store: no copy until encode() writes it into the reply buffer
        Nil,
        Error(Code, String),
    }

    /// v1 grammar, unchanged: tokens separated by ASCII whitespace, command names case-insensitive.
    pub fn parse(line: &str) -> Result<Request, (Code, String)> {
        let mut t = line.split_ascii_whitespace();
        let Some(cmd) = t.next() else {
            return Err((Code::Empty, "empty command".to_string()));
        };
        let (a, b, extra) = (t.next(), t.next(), t.next());
        let args = [a, b, extra].iter().filter(|x| x.is_some()).count(); // 3 means "3 or more"
        let wrong = || Err((Code::WrongArity, format!("wrong number of arguments for '{}'", cmd.to_ascii_uppercase())));
        let is = |name: &str| cmd.eq_ignore_ascii_case(name);
        if is("PING") {
            if args == 0 { Ok(Request::Ping) } else { wrong() }
        } else if is("GET") {
            match (args, a) { (1, Some(k)) => Ok(Request::Get(k.into())), _ => wrong() }
        } else if is("SET") {
            match (args, a, b) { (2, Some(k), Some(v)) => Ok(Request::Set(k.into(), v.into())), _ => wrong() }
        } else if is("DEL") {
            match (args, a) { (1, Some(k)) => Ok(Request::Del(k.into())), _ => wrong() }
        } else {
            Err((Code::UnknownCommand, format!("unknown command '{cmd}'")))
        }
    }

    /// SET -> +OK; GET -> $value or _; DEL -> +OK if something was removed, _ if the key was absent.
    pub fn execute<S: KvStore + ?Sized>(store: &S, req: Request) -> Response {
        match req {
            Request::Ping => Response::Pong,
            Request::Get(k) => store.get_shared(&k).map_or(Response::Nil, Response::Value),
            Request::Set(k, v) => {
                store.put(k, v);
                Response::Ok
            }
            Request::Del(k) => {
                if store.delete(&k).is_some() { Response::Ok } else { Response::Nil }
            }
        }
    }

    /// One request line (without its '\n') to one reply. Synchronous: the store never awaits.
    pub fn handle_line<S: KvStore + ?Sized>(store: &S, line: &[u8]) -> Response {
        match std::str::from_utf8(line) {
            Ok(text) => match parse(text) {
                Ok(req) => execute(store, req),
                Err((code, detail)) => Response::Error(code, detail),
            },
            Err(_) => Response::Error(Code::InvalidUtf8, "request is not valid UTF-8".into()),
        }
    }

    impl Response {
        pub fn encode(&self, dst: &mut BytesMut) {
            match self {
                Response::Pong => dst.put_slice(b"+PONG\n"),
                Response::Ok => dst.put_slice(b"+OK\n"),
                Response::Value(v) => {
                    dst.reserve(v.len() + 2);
                    dst.put_u8(b'$');
                    dst.put_slice(v);
                    dst.put_u8(b'\n');
                }
                Response::Nil => dst.put_slice(b"_\n"),
                Response::Error(code, detail) => {
                    dst.put_slice(b"-ERR ");
                    dst.put_slice(code.as_str().as_bytes());
                    dst.put_u8(b' ');
                    dst.put_slice(detail.as_bytes());
                    dst.put_u8(b'\n');
                }
            }
        }
    }
}

/// Framing: bytes in, request lines out; replies in, bytes out.
mod codec {
    use crate::protocol::Response;
    use bytes::BytesMut;
    use std::io;
    use tokio_util::codec::{Decoder, Encoder};

    #[derive(Debug)]
    pub enum FrameError {
        TooLong,
        Io, // the server only needs to know that the client is gone, not why
    }

    impl From<io::Error> for FrameError {
        fn from(_: io::Error) -> FrameError {
            FrameError::Io
        }
    }

    /// Splits the byte stream into lines. Bytes arrive in arbitrary pieces: one read may hold half a request,
    /// or three and a half. `decode` returns Ok(None) until a whole line is buffered; FramedRead then reads more.
    pub struct LineCodec {
        max_line: usize,
        scanned: usize, // bytes of the buffer already searched for '\n'
    }

    impl LineCodec {
        pub fn new(max_line: usize) -> LineCodec {
            LineCodec { max_line, scanned: 0 }
        }
    }

    impl Decoder for LineCodec {
        type Item = BytesMut;
        type Error = FrameError;

        fn decode(&mut self, src: &mut BytesMut) -> Result<Option<BytesMut>, FrameError> {
            // Resume the search where the last call stopped: each byte is scanned once, however it arrives.
            match memchr::memchr(b'\n', &src[self.scanned..]) {
                Some(i) => {
                    let end = self.scanned + i; // index of the '\n'
                    self.scanned = 0;
                    if end > self.max_line {
                        return Err(FrameError::TooLong);
                    }
                    let mut line = src.split_to(end + 1); // the line and its '\n' leave the buffer, no copy
                    line.truncate(end);
                    if line.last() == Some(&b'\r') {
                        line.truncate(end - 1);
                    }
                    Ok(Some(line))
                }
                None if src.len() > self.max_line => Err(FrameError::TooLong),
                None => {
                    self.scanned = src.len();
                    Ok(None)
                }
            }
        }
    }

    pub struct ReplyCodec;

    impl Encoder<Response> for ReplyCodec {
        type Error = io::Error;

        fn encode(&mut self, item: Response, dst: &mut BytesMut) -> io::Result<()> {
            item.encode(dst);
            Ok(())
        }
    }
}

mod server {
    use crate::codec::{FrameError, LineCodec, ReplyCodec};
    use crate::protocol::{self, Code, Response};
    use crate::store::KvStore;
    use bytes::BytesMut;
    use futures::{SinkExt, StreamExt};
    use std::io;
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
    use tokio::net::tcp::OwnedWriteHalf;
    use tokio::net::{TcpListener, TcpSocket, TcpStream};
    use tokio::sync::Semaphore;
    use tokio::task::{JoinError, JoinSet};
    use tokio::time::timeout;
    use tokio_util::codec::{FramedRead, FramedWrite};
    use tokio_util::sync::CancellationToken;

    #[derive(Clone, Debug)]
    pub struct Config {
        pub max_connections: usize, // beyond this, refuse with -ERR BUSY (don't queue)
        pub idle_timeout: Duration, // no complete request for this long: close
        pub write_timeout: Duration, // one reply can't be handed to the kernel for this long: close
        pub max_line: usize, // longest request line
        pub max_unflushed: usize, // reply bytes buffered before a write is forced (and awaited)
        pub drain_timeout: Duration, // after shutdown, abort connections still running after this long
        pub backlog: u32, // kernel accept queue (TcpListener::bind would use 128)
    }

    /// Per-server counters (v1 used process-wide statics: two servers in one process shared them).
    #[derive(Default, Debug)]
    pub struct Stats {
        pub accepted: AtomicU64,
        pub refused_busy: AtomicU64,
        pub requests: AtomicU64,
        pub closed_by_client: AtomicU64,
        pub closed_idle: AtomicU64,
        pub closed_slow_reader: AtomicU64,
        pub closed_line_too_long: AtomicU64,
        pub closed_shutdown: AtomicU64,
        pub panicked: AtomicU64,
        pub active: AtomicUsize,
        pub peak: AtomicUsize,
    }

    impl Stats {
        pub fn line(&self) -> String {
            let g = |a: &AtomicU64| a.load(Relaxed);
            format!(
                "accepted {} | refused BUSY {} | requests {} | closed: client {}, idle {}, slow reader {}, line too long {}, shutdown {} | panics {} | peak connections {}",
                g(&self.accepted), g(&self.refused_busy), g(&self.requests), g(&self.closed_by_client), g(&self.closed_idle),
                g(&self.closed_slow_reader), g(&self.closed_line_too_long), g(&self.closed_shutdown), g(&self.panicked),
                self.peak.load(Relaxed)
            )
        }
    }

    #[derive(Debug)]
    pub struct Report {
        pub drained: bool, // every connection finished before the drain deadline
        pub aborted: usize, // connections still running at the deadline, aborted
    }

    pub struct Server<S> {
        listener: TcpListener,
        store: Arc<S>,
        config: Arc<Config>,
        stats: Arc<Stats>,
    }

    impl<S: KvStore + Send + Sync + 'static> Server<S> {
        pub async fn bind(addr: &str, store: Arc<S>, config: Config) -> io::Result<Server<S>> {
            let addr: SocketAddr = addr.parse().map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
            let socket = if addr.is_ipv4() { TcpSocket::new_v4()? } else { TcpSocket::new_v6()? };
            socket.set_reuseaddr(true)?; // restart without waiting out TIME_WAIT on the port
            socket.bind(addr)?;
            let listener = socket.listen(config.backlog)?; // our accept queue, not the default 128
            Ok(Server { listener, store, config: Arc::new(config), stats: Arc::new(Stats::default()) })
        }

        pub fn local_addr(&self) -> SocketAddr {
            self.listener.local_addr().unwrap()
        }

        pub fn stats(&self) -> Arc<Stats> {
            Arc::clone(&self.stats)
        }

        /// Accept until `shutdown` is cancelled. Then drain: each connection finishes the request it is
        /// serving, says SHUTTING_DOWN, and closes; connections still running at the deadline are aborted.
        pub async fn run(self, shutdown: CancellationToken) -> Report {
            let limit = Arc::new(Semaphore::new(self.config.max_connections));
            let mut conns = JoinSet::new(); // owns every connection task: none can outlive run()
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown.cancelled() => break,
                    Some(done) = conns.join_next() => record(&self.stats, done), // reap finished connections
                    accepted = self.listener.accept() => {
                        let stream = match accepted {
                            Ok((stream, _)) => stream,
                            Err(_) => {
                                // EMFILE, ENFILE, ECONNABORTED: back off instead of spinning on the error
                                tokio::time::sleep(Duration::from_millis(10)).await;
                                continue;
                            }
                        };
                        match Arc::clone(&limit).try_acquire_owned() {
                            Ok(permit) => {
                                self.stats.accepted.fetch_add(1, Relaxed);
                                let (store, config) = (Arc::clone(&self.store), Arc::clone(&self.config));
                                let (stats, stop) = (Arc::clone(&self.stats), shutdown.clone());
                                conns.spawn(async move {
                                    let _permit = permit; // released when the connection ends, however it ends
                                    let _active = ActiveGuard::new(&stats);
                                    serve(stream, &*store, &config, &stats, &stop).await;
                                });
                            }
                            Err(_) => {
                                self.stats.refused_busy.fetch_add(1, Relaxed);
                                // Refuse in a task of its own: a refused client that doesn't read can't stall accept.
                                conns.spawn(refuse(stream));
                            }
                        }
                    }
                }
            }
            drop(self.listener); // stop accepting: from here on the kernel refuses new connections
            let stats = Arc::clone(&self.stats);
            let drained = timeout(self.config.drain_timeout, async {
                while let Some(done) = conns.join_next().await {
                    record(&stats, done);
                }
            })
            .await
            .is_ok();
            let aborted = conns.len();
            conns.shutdown().await; // abort the stragglers and wait until they are gone
            Report { drained, aborted }
        }
    }

    fn record(stats: &Stats, done: Result<(), JoinError>) {
        if let Err(e) = done {
            if e.is_panic() {
                stats.panicked.fetch_add(1, Relaxed); // one connection lost; the server lives on
            }
        }
    }

    struct ActiveGuard<'a>(&'a Stats);

    impl<'a> ActiveGuard<'a> {
        fn new(stats: &'a Stats) -> ActiveGuard<'a> {
            let now = stats.active.fetch_add(1, Relaxed) + 1;
            stats.peak.fetch_max(now, Relaxed);
            ActiveGuard(stats)
        }
    }

    impl Drop for ActiveGuard<'_> {
        fn drop(&mut self) {
            self.0.active.fetch_sub(1, Relaxed);
        }
    }

    enum Closed {
        ByClient,
        Idle,
        SlowReader,
        LineTooLong,
        Shutdown,
    }

    type Replies = FramedWrite<OwnedWriteHalf, ReplyCodec>;

    async fn serve<S: KvStore + ?Sized>(stream: TcpStream, store: &S, config: &Config, stats: &Stats, stop: &CancellationToken) {
        let _ = stream.set_nodelay(true); // replies are small and latency-bound: don't let Nagle hold them
        let (rd, wr) = stream.into_split();
        let mut requests = FramedRead::new(rd, LineCodec::new(config.max_line));
        let mut replies = FramedWrite::new(wr, ReplyCodec);
        replies.set_backpressure_boundary(config.max_unflushed);
        let why = loop {
            // Between requests is the only place where shutdown or idleness may stop a connection.
            let next = tokio::select! {
                biased;
                _ = stop.cancelled() => {
                    let _ = send_now(&mut replies, config, Response::Error(Code::ShuttingDown, "server is shutting down".into())).await;
                    break Closed::Shutdown;
                }
                next = timeout(config.idle_timeout, requests.next()) => next, // FramedRead::next is cancel safe
            };
            let line = match next {
                Err(_elapsed) => break Closed::Idle,
                Ok(None) => break Closed::ByClient,
                Ok(Some(Err(FrameError::TooLong))) => {
                    let detail = format!("request longer than {} bytes", config.max_line);
                    let _ = send_now(&mut replies, config, Response::Error(Code::LineTooLong, detail)).await;
                    let _ = replies.close().await; // FIN: no more replies
                    linger(requests.into_inner()).await;
                    break Closed::LineTooLong;
                }
                Ok(Some(Err(FrameError::Io))) => break Closed::ByClient, // reset, or EOF inside a line
                Ok(Some(Ok(line))) => line,
            };
            stats.requests.fetch_add(1, Relaxed);
            let reply = protocol::handle_line(store, &line); // no lock outlives this call: nothing to hold across .await
            // Flush before we might wait for input: that is, unless another complete request is already buffered.
            let more_waiting = memchr::memchr(b'\n', requests.read_buffer()).is_some();
            let wrote = timeout(config.write_timeout, async {
                replies.feed(reply).await?; // buffers; writes (and waits) first if max_unflushed is reached
                if !more_waiting {
                    replies.flush().await?;
                }
                Ok::<(), io::Error>(())
            })
            .await;
            match wrote {
                Ok(Ok(())) => {}
                Ok(Err(_)) => break Closed::ByClient,
                Err(_elapsed) => break Closed::SlowReader, // the client isn't reading its replies
            }
        };
        let counter = match why {
            Closed::ByClient => &stats.closed_by_client,
            Closed::Idle => &stats.closed_idle,
            Closed::SlowReader => &stats.closed_slow_reader,
            Closed::LineTooLong => &stats.closed_line_too_long,
            Closed::Shutdown => &stats.closed_shutdown,
        };
        counter.fetch_add(1, Relaxed);
    }

    async fn send_now(replies: &mut Replies, config: &Config, r: Response) -> Result<(), ()> {
        match timeout(config.write_timeout, replies.send(r)).await {
            Ok(Ok(())) => Ok(()),
            _ => Err(()),
        }
    }

    /// Closing a socket with unread input makes the kernel send RST, which can destroy a reply still in flight.
    /// So after the last reply and our FIN, read and discard what the client is still sending, briefly.
    async fn linger(mut rd: impl AsyncRead + Unpin) {
        let mut discard = tokio::io::sink();
        let _ = timeout(Duration::from_millis(100), tokio::io::copy(&mut (&mut rd).take(1 << 20), &mut discard)).await;
    }

    async fn refuse(mut stream: TcpStream) {
        let mut out = BytesMut::new();
        Response::Error(Code::Busy, "connection limit reached".into()).encode(&mut out);
        if timeout(Duration::from_millis(100), stream.write_all(&out)).await.is_ok() {
            let _ = stream.shutdown().await;
            linger(stream).await;
        }
    }
}

use server::{Config, Server};
use std::net::SocketAddr;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::{Duration, Instant};
use store::{KvStore, ShardedStore};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

/// A small async client: one connection, replies read one line at a time.
struct Client {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
}

impl Client {
    async fn connect(addr: SocketAddr) -> Client {
        let (rd, wr) = TcpStream::connect(addr).await.unwrap().into_split();
        Client { reader: BufReader::new(rd), writer: wr }
    }

    async fn send_raw(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes).await;
    }

    /// One reply line, or "<closed>" if the server closed (or reset) the connection.
    async fn read_reply(&mut self) -> String {
        let mut line = String::new();
        match self.reader.read_line(&mut line).await {
            Ok(0) | Err(_) => "<closed>".to_string(),
            Ok(_) => line.trim_end_matches('\n').to_string(),
        }
    }

    async fn call(&mut self, line: &str) -> String {
        self.send_raw(format!("{line}\n").as_bytes()).await;
        self.read_reply().await
    }
}

fn os_threads() -> usize {
    std::fs::read_dir("/proc/self/task").map(|d| d.count()).unwrap_or(0)
}

/// Each connection needs a descriptor on both ends here; raise the soft limit to the hard limit.
fn raise_fd_limit() -> u64 {
    let mut lim = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: getrlimit/setrlimit only read and write the struct we pass; RLIMIT_NOFILE is a valid resource.
    unsafe {
        libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim);
        lim.rlim_cur = lim.rlim_max;
        libc::setrlimit(libc::RLIMIT_NOFILE, &lim);
        libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim);
    }
    lim.rlim_cur as u64
}

#[tokio::main]
async fn main() {
    let fd_limit = raise_fd_limit();
    let store = Arc::new(ShardedStore::new(16));
    let config = Config {
        max_connections: 2_000,
        idle_timeout: Duration::from_secs(10), // longer than any client here waits between requests
        write_timeout: Duration::from_secs(1),
        max_line: 64 * 1024,
        max_unflushed: 64 * 1024,
        drain_timeout: Duration::from_secs(1),
        backlog: 2_048,
    };
    let server = Server::bind("127.0.0.1:0", Arc::clone(&store), config).await.unwrap();
    let (addr, stats) = (server.local_addr(), server.stats());
    let shutdown = CancellationToken::new();
    let running = tokio::spawn(server.run(shutdown.clone()));

    println!("-- one session");
    let mut c = Client::connect(addr).await;
    for cmd in ["PING", "SET user:1 ada", "GET user:1", "GET user:2", "DEL user:1", "DEL user:1", "BOGUS 1", "GET", ""] {
        println!("> {cmd:<16} < {}", c.call(cmd).await);
    }

    println!("-- one request split across three TCP writes, 20 ms apart, then a pipelined GET");
    c.send_raw(b"SE").await;
    sleep(Duration::from_millis(20)).await;
    c.send_raw(b"T split ").await;
    sleep(Duration::from_millis(20)).await;
    c.send_raw(b"works\r\nGET split\n").await;
    println!("{:?}", [c.read_reply().await, c.read_reply().await]);

    println!("-- a 70,000-byte request line");
    let reply = c.call(&"A".repeat(70_000)).await;
    println!("< {reply}");
    println!("< {} (the server hung up)", c.read_reply().await);

    // Many concurrent connections: one task each, no worker-per-connection limit.
    let clients_n: usize = if fd_limit >= 2_300 { 1_000 } else { 400 };
    println!("-- {clients_n} concurrent connections x 100 commands (fd limit {fd_limit})");
    let threads_before = os_threads();
    let all_open = Arc::new(tokio::sync::Barrier::new(clients_n));
    let t = Instant::now();
    let tasks: Vec<_> = (0..clients_n)
        .map(|id| {
            let all_open = Arc::clone(&all_open);
            tokio::spawn(async move {
                let mut c = Client::connect(addr).await;
                let mut correct = c.call("PING").await == "+PONG";
                all_open.wait().await; // every connection is open before any does real work
                for i in 0..50 {
                    correct &= c.call(&format!("SET c{id}:k{i} v{i}")).await == "+OK";
                }
                for i in 0..50 {
                    correct &= c.call(&format!("GET c{id}:k{i}")).await == format!("$v{i}");
                }
                correct
            })
        })
        .collect();
    let mut all_correct = true;
    for task in tasks {
        all_correct &= task.await.unwrap();
    }
    let secs = t.elapsed().as_secs_f64();
    println!("all {} replies correct: {all_correct}; {:.0} commands/s, clients and server in one process (one run)", clients_n * 101, (clients_n * 101) as f64 / secs);
    println!("peak connections served at once: {}; OS threads in the process: {} before, {} during", stats.peak.load(Relaxed), threads_before, os_threads());

    drop(c);
    shutdown.cancel();
    let report = running.await.unwrap();
    println!("server 1 stopped: drained {}, aborted {}", report.drained, report.aborted);
    println!("server 1 stats: {}", stats.line());

    // The limits, one at a time, on a small server.
    println!("-- limits: max_connections 2, idle 300 ms, write timeout 300 ms, drain 200 ms");
    let config = Config {
        max_connections: 2,
        idle_timeout: Duration::from_millis(300),
        write_timeout: Duration::from_millis(300),
        max_line: 1_024,
        max_unflushed: 64 * 1024,
        drain_timeout: Duration::from_millis(200),
        backlog: 128,
    };
    let server = Server::bind("127.0.0.1:0", Arc::clone(&store), config).await.unwrap();
    let (addr, stats) = (server.local_addr(), server.stats());
    let shutdown = CancellationToken::new();
    let running = tokio::spawn(server.run(shutdown.clone()));

    let (mut a, mut b) = (Client::connect(addr).await, Client::connect(addr).await);
    println!("client A: {}   client B: {}", a.call("PING").await, b.call("PING").await);
    let mut third = Client::connect(addr).await;
    println!("client C: {}   then {}", third.read_reply().await, third.read_reply().await);
    drop((a, b));
    sleep(Duration::from_millis(50)).await; // the server notices both EOFs and releases their permits

    let mut idle = Client::connect(addr).await;
    let t = Instant::now();
    let r = idle.read_reply().await;
    println!("idle client: {r} after {} ms", t.elapsed().as_millis() / 10 * 10);

    store.put(b"big".to_vec(), vec![b'x'; 64 * 1024]);
    let mut slow = Client::connect(addr).await;
    slow.send_raw("GET big\n".repeat(2_000).as_bytes()).await; // 16 KB of requests; never reads a reply
    let t = Instant::now();
    while stats.closed_slow_reader.load(Relaxed) == 0 && t.elapsed() < Duration::from_secs(5) {
        sleep(Duration::from_millis(5)).await;
    }
    println!("slow reader: disconnected after {} ms", t.elapsed().as_millis() / 10 * 10);
    drop(slow);

    let mut last = Client::connect(addr).await;
    println!("last client: {}", last.call("PING").await);
    shutdown.cancel();
    println!("last client, after shutdown: {}   then {}", last.read_reply().await, last.read_reply().await);
    let report = running.await.unwrap();
    println!("server 2 stopped: drained {}, aborted {}", report.drained, report.aborted);
    println!("server 2 stats: {}", stats.line());
    let lens = store.shard_lens();
    println!(
        "store: {} keys across 16 shards (smallest {}, largest {}), poison recoveries {}",
        store.len(),
        lens.iter().min().unwrap(),
        lens.iter().max().unwrap(),
        store.poison_recoveries()
    );
}

#[cfg(test)]
mod tests {
    use super::codec::{FrameError, LineCodec};
    use super::protocol::{handle_line, parse, Code, Request, Response};
    use super::*;
    use bytes::BytesMut;
    use tokio_util::codec::Decoder;

    fn test_config() -> Config {
        Config {
            max_connections: 4,
            idle_timeout: Duration::from_millis(200),
            write_timeout: Duration::from_millis(200),
            max_line: 64,
            max_unflushed: 16 * 1024,
            drain_timeout: Duration::from_millis(200),
            backlog: 128,
        }
    }

    async fn start<S: KvStore + Send + Sync + 'static>(store: Arc<S>, config: Config)
        -> (SocketAddr, Arc<server::Stats>, CancellationToken, tokio::task::JoinHandle<server::Report>) {
        let server = Server::bind("127.0.0.1:0", store, config).await.unwrap();
        let (addr, stats, stop) = (server.local_addr(), server.stats(), CancellationToken::new());
        let running = tokio::spawn(server.run(stop.clone()));
        (addr, stats, stop, running)
    }

    #[test]
    fn frames_split_across_reads_are_reassembled() {
        let mut codec = LineCodec::new(64);
        let mut buf = BytesMut::new();
        let mut frames = Vec::new();
        for chunk in [&b"SE"[..], b"T k v\r\nGE", b"T k\n"] {
            buf.extend_from_slice(chunk); // what one read() delivered
            while let Some(frame) = codec.decode(&mut buf).unwrap() {
                frames.push(String::from_utf8(frame.to_vec()).unwrap());
            }
        }
        assert_eq!(frames, ["SET k v", "GET k"]);
        assert!(buf.is_empty());
    }

    #[test]
    fn long_lines_are_rejected_with_or_without_a_newline() {
        let mut codec = LineCodec::new(8);
        assert!(matches!(codec.decode(&mut BytesMut::from(&b"GET 12345\n"[..])), Err(FrameError::TooLong)));
        let mut codec = LineCodec::new(8);
        assert!(matches!(codec.decode(&mut BytesMut::from(&b"GET 123456789"[..])), Err(FrameError::TooLong)));
        let mut codec = LineCodec::new(8);
        assert!(matches!(codec.decode(&mut BytesMut::from(&b"GET 1234\n"[..])), Ok(Some(_))));
    }

    #[test]
    fn errors_carry_stable_codes() {
        assert_eq!(parse("get k"), Ok(Request::Get(b"k".to_vec())));
        assert_eq!(parse("GET").unwrap_err().0, Code::WrongArity);
        assert_eq!(parse("FLUSHALL").unwrap_err().0, Code::UnknownCommand);
        let store = ShardedStore::new(1);
        assert_eq!(handle_line(&store, b"\xff"), Response::Error(Code::InvalidUtf8, "request is not valid UTF-8".into()));
        let mut out = BytesMut::new();
        Response::Error(Code::Busy, "connection limit reached".into()).encode(&mut out);
        assert_eq!(&out[..], b"-ERR BUSY connection limit reached\n");
    }

    #[test]
    fn store_semantics() {
        let s = ShardedStore::new(4);
        assert_eq!(s.put(b"k".to_vec(), b"1".to_vec()), None);
        assert_eq!(s.put(b"k".to_vec(), b"2".to_vec()), Some(b"1".to_vec()));
        assert_eq!(s.get(b"k"), Some(b"2".to_vec()));
        assert_eq!(s.delete(b"k"), Some(b"2".to_vec()));
        assert_eq!(s.delete(b"k"), None);
        assert_eq!(s.len(), 0);
    }

    #[tokio::test]
    async fn over_the_limit_is_refused_with_busy_and_capacity_comes_back() {
        let config = Config { max_connections: 1, ..test_config() };
        let (addr, stats, stop, running) = start(Arc::new(ShardedStore::new(4)), config).await;
        let mut first = Client::connect(addr).await;
        assert_eq!(first.call("PING").await, "+PONG");
        let mut second = Client::connect(addr).await;
        assert_eq!(second.read_reply().await, "-ERR BUSY connection limit reached");
        assert_eq!(second.read_reply().await, "<closed>");
        drop(first);
        sleep(Duration::from_millis(50)).await;
        let mut third = Client::connect(addr).await;
        assert_eq!(third.call("PING").await, "+PONG");
        assert_eq!(stats.refused_busy.load(Relaxed), 1);
        stop.cancel();
        running.await.unwrap();
    }

    #[tokio::test]
    async fn idle_connections_are_closed() {
        let (addr, stats, stop, running) = start(Arc::new(ShardedStore::new(4)), test_config()).await;
        let mut c = Client::connect(addr).await;
        c.send_raw(b"PIN").await; // half a request, then silence
        let t = Instant::now();
        assert_eq!(c.read_reply().await, "<closed>");
        assert!(t.elapsed() >= Duration::from_millis(190), "closed too early: {:?}", t.elapsed());
        assert_eq!(stats.closed_idle.load(Relaxed), 1);
        stop.cancel();
        running.await.unwrap();
    }

    #[tokio::test]
    async fn a_client_that_does_not_read_is_disconnected() {
        let store = Arc::new(ShardedStore::new(4));
        store.put(b"big".to_vec(), vec![b'x'; 64 * 1024]);
        let (addr, stats, stop, running) = start(Arc::clone(&store), test_config()).await;
        let mut c = Client::connect(addr).await;
        c.send_raw("GET big\n".repeat(2_000).as_bytes()).await;
        let t = Instant::now();
        while stats.closed_slow_reader.load(Relaxed) == 0 && t.elapsed() < Duration::from_secs(5) {
            sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(stats.closed_slow_reader.load(Relaxed), 1);
        stop.cancel();
        let report = running.await.unwrap();
        assert!(report.drained);
    }

    #[tokio::test]
    async fn shutdown_says_goodbye_between_requests_and_drains() {
        let (addr, stats, stop, running) = start(Arc::new(ShardedStore::new(4)), test_config()).await;
        let mut c = Client::connect(addr).await;
        assert_eq!(c.call("SET a 1").await, "+OK");
        stop.cancel();
        assert_eq!(c.read_reply().await, "-ERR SHUTTING_DOWN server is shutting down");
        assert_eq!(c.read_reply().await, "<closed>");
        let report = running.await.unwrap();
        assert!(report.drained && report.aborted == 0, "{report:?}");
        assert_eq!(stats.closed_shutdown.load(Relaxed), 1);
        assert!(TcpStream::connect(addr).await.is_err(), "the listener should be closed");
    }

    #[tokio::test]
    async fn the_drain_deadline_aborts_connections_stuck_writing() {
        let store = Arc::new(ShardedStore::new(4));
        store.put(b"big".to_vec(), vec![b'x'; 64 * 1024]);
        let config = Config { write_timeout: Duration::from_secs(30), ..test_config() };
        let (addr, _stats, stop, running) = start(Arc::clone(&store), config).await;
        let mut c = Client::connect(addr).await;
        c.send_raw("GET big\n".repeat(2_000).as_bytes()).await; // the server blocks writing replies nobody reads
        sleep(Duration::from_millis(100)).await;
        stop.cancel();
        let t = Instant::now();
        let report = running.await.unwrap();
        assert!(!report.drained && report.aborted == 1, "{report:?}");
        assert!(t.elapsed() < Duration::from_millis(1_000));
    }

    struct Exploding(ShardedStore);

    impl KvStore for Exploding {
        fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
            assert!(key != b"boom", "bug: GET boom");
            self.0.get(key)
        }
        fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Option<Vec<u8>> {
            self.0.put(key, value)
        }
        fn delete(&self, key: &[u8]) -> Option<Vec<u8>> {
            self.0.delete(key)
        }
    }

    #[tokio::test]
    async fn a_panic_costs_one_connection_not_the_server() {
        let (addr, stats, stop, running) = start(Arc::new(Exploding(ShardedStore::new(4))), test_config()).await;
        let mut victim = Client::connect(addr).await;
        let mut bystander = Client::connect(addr).await;
        assert_eq!(victim.call("GET boom").await, "<closed>");
        assert_eq!(bystander.call("PING").await, "+PONG");
        sleep(Duration::from_millis(20)).await;
        assert_eq!(stats.panicked.load(Relaxed), 1);
        assert_eq!(stats.active.load(Relaxed), 1, "the panicked connection released its slot");
        stop.cancel();
        running.await.unwrap();
    }

    #[tokio::test]
    async fn v1_poisoning_policy_still_serves_the_shard() {
        let store = Arc::new(ShardedStore::new(4));
        store.put(b"k".to_vec(), b"v".to_vec());
        store.poison_shard_of(b"k");
        let (addr, _stats, stop, running) = start(Arc::clone(&store), test_config()).await;
        let mut c = Client::connect(addr).await;
        assert_eq!(c.call("GET k").await, "$v");
        assert_eq!(store.poison_recoveries(), 1);
        stop.cancel();
        running.await.unwrap();
    }
}
