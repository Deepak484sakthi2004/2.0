// verify: debug ok
// verify: debug test
// verify: release ok
//! Ferrite v1: a concurrent in-memory key-value store (the library) behind a line protocol (the server).
//!
//! - store:    the `KvStore` trait (the contract every later Ferrite version keeps) and `ShardedStore`,
//!             N shards of RwLock<HashMap<Vec<u8>, Vec<u8>>>, shard = keyed hash(key) % N.
//! - protocol: v1 wire format. Requests: PING | GET <key> | SET <key> <value> | DEL <key>, one per line.
//!             Responses: +PONG | +OK | $<value> | _ (nil) | -ERR <message>, each terminated by '\n'.
//! - pool:     the worker pool from Project L3, unchanged (in a workspace it would be a shared crate).
//! - server:   one worker per connection, idle timeout, line-length limit, pipelined replies batched,
//!             -ERR server busy when the queue is full, graceful shutdown.

/// The worker pool from Project L3 (listing project-03-http-server.rs), unchanged.
mod pool {
    use std::collections::VecDeque;
    use std::panic::{self, AssertUnwindSafe};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::thread::{self, JoinHandle};

    #[derive(Default)]
    pub struct Stats {
        pub completed: AtomicU64,
        pub panicked: AtomicU64,
        pub rejected: AtomicU64,
    }

    struct Queue<T> {
        items: VecDeque<T>,
        closed: bool,
    }

    struct Shared<T> {
        queue: Mutex<Queue<T>>,
        item_ready: Condvar,
        capacity: usize,
        stats: Stats,
    }

    /// `workers` threads run `handler(item)` for each submitted item; at most `capacity` items wait.
    pub struct WorkerPool<T: Send + 'static> {
        shared: Arc<Shared<T>>,
        workers: Vec<JoinHandle<()>>,
    }

    impl<T: Send + 'static> WorkerPool<T> {
        pub fn new<H>(workers: usize, capacity: usize, name: &str, handler: H) -> WorkerPool<T>
        where
            H: Fn(T) + Send + Sync + 'static,
        {
            assert!(workers > 0 && capacity > 0);
            let shared = Arc::new(Shared {
                queue: Mutex::new(Queue { items: VecDeque::with_capacity(capacity), closed: false }),
                item_ready: Condvar::new(),
                capacity,
                stats: Stats::default(),
            });
            let handler = Arc::new(handler);
            let workers = (0..workers)
                .map(|i| {
                    let (shared, handler) = (Arc::clone(&shared), Arc::clone(&handler));
                    thread::Builder::new()
                        .name(format!("{name}-{i}"))
                        .spawn(move || worker_loop(&shared, &*handler))
                        .expect("failed to spawn worker thread")
                })
                .collect();
            WorkerPool { shared, workers }
        }

        /// Queue `item` if there is room. If not, hand it back: the caller decides how to refuse.
        pub fn try_submit(&self, item: T) -> Result<(), T> {
            let mut q = self.shared.queue.lock().unwrap();
            if q.closed || q.items.len() >= self.shared.capacity {
                self.shared.stats.rejected.fetch_add(1, Ordering::Relaxed);
                return Err(item);
            }
            q.items.push_back(item);
            drop(q); // unlock first: the woken worker can take the lock at once
            self.shared.item_ready.notify_one();
            Ok(())
        }

        /// Graceful shutdown, then the final (completed, panicked, rejected) counts.
        pub fn shutdown(mut self) -> (u64, u64, u64) {
            self.close_and_join();
            let s = &self.shared.stats;
            (s.completed.load(Ordering::Relaxed), s.panicked.load(Ordering::Relaxed), s.rejected.load(Ordering::Relaxed))
        }

        /// Refuse new items, let the workers drain the queue, wait for all of them.
        fn close_and_join(&mut self) {
            self.shared.queue.lock().unwrap().closed = true;
            self.shared.item_ready.notify_all();
            for w in self.workers.drain(..) {
                let _ = w.join();
            }
        }
    }

    fn worker_loop<T, H: Fn(T)>(shared: &Shared<T>, handler: &H) {
        loop {
            let item = {
                let q = shared.queue.lock().unwrap();
                let mut q = shared.item_ready.wait_while(q, |q| q.items.is_empty() && !q.closed).unwrap();
                match q.items.pop_front() {
                    Some(item) => item,
                    None => return, // closed and drained: this worker is finished
                }
            }; // the queue lock is released BEFORE the handler runs: a panic can't poison it
            match panic::catch_unwind(AssertUnwindSafe(|| handler(item))) {
                Ok(()) => shared.stats.completed.fetch_add(1, Ordering::Relaxed),
                Err(_) => shared.stats.panicked.fetch_add(1, Ordering::Relaxed), // the worker lives on
            };
        }
    }

    impl<T: Send + 'static> Drop for WorkerPool<T> {
        /// Dropping the pool is a graceful shutdown too (a no-op if shutdown() already ran).
        fn drop(&mut self) {
            self.close_and_join();
        }
    }
}

mod store {
    use std::collections::HashMap;
    use std::hash::{BuildHasher, RandomState};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

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
    }

    type Map = HashMap<Vec<u8>, Vec<u8>>;

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
            self.read(self.shard(key)).get(key).cloned() // the copy happens under the read lock
        }

        fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Option<Vec<u8>> {
            let shard = self.shard(&key);
            self.write(shard).insert(key, value)
        }

        fn delete(&self, key: &[u8]) -> Option<Vec<u8>> {
            self.write(self.shard(key)).remove(key)
        }
    }
}

mod protocol {
    use crate::store::KvStore;
    use std::io::{self, Write};

    #[derive(Debug, PartialEq)]
    pub enum Request {
        Ping,
        Get(Vec<u8>),
        Set(Vec<u8>, Vec<u8>),
        Del(Vec<u8>),
    }

    #[derive(Debug, PartialEq)]
    pub enum Response {
        Pong,
        Ok,
        Value(Vec<u8>),
        Nil,
        Error(String),
    }

    /// v1 grammar: tokens separated by ASCII whitespace (so keys and values can't contain any);
    /// command names are case-insensitive.
    pub fn parse(line: &str) -> Result<Request, String> {
        let mut t = line.split_ascii_whitespace();
        let Some(cmd) = t.next() else {
            return Err("empty command".to_string());
        };
        let (a, b, extra) = (t.next(), t.next(), t.next());
        let args = [a, b, extra].iter().filter(|x| x.is_some()).count(); // 3 means "3 or more"
        let wrong = || Err(format!("wrong number of arguments for '{}'", cmd.to_ascii_uppercase()));
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
            Err(format!("unknown command '{cmd}'"))
        }
    }

    impl Response {
        pub fn write_to(&self, out: &mut impl Write) -> io::Result<()> {
            match self {
                Response::Pong => out.write_all(b"+PONG\n"),
                Response::Ok => out.write_all(b"+OK\n"),
                Response::Value(v) => {
                    out.write_all(b"$")?;
                    out.write_all(v)?;
                    out.write_all(b"\n")
                }
                Response::Nil => out.write_all(b"_\n"),
                Response::Error(msg) => writeln!(out, "-ERR {msg}"),
            }
        }
    }

    /// SET -> +OK; GET -> $value or _; DEL -> +OK if something was removed, _ if the key was absent.
    pub fn execute<S: KvStore + ?Sized>(store: &S, req: Request) -> Response {
        match req {
            Request::Ping => Response::Pong,
            Request::Get(k) => store.get(&k).map_or(Response::Nil, Response::Value),
            Request::Set(k, v) => {
                store.put(k, v);
                Response::Ok
            }
            Request::Del(k) => {
                if store.delete(&k).is_some() { Response::Ok } else { Response::Nil }
            }
        }
    }
}


mod server {
    use crate::pool::WorkerPool;
    use crate::protocol::{self, Response};
    use crate::store::KvStore;
    use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    pub struct Config {
        pub workers: usize,
        pub queue_capacity: usize,
        pub idle_timeout: Duration,
        pub max_line: usize,
    }

    /// Connections being served right now, and the most ever served at once.
    pub static ACTIVE: AtomicUsize = AtomicUsize::new(0);
    pub static PEAK: AtomicUsize = AtomicUsize::new(0);

    pub struct Server {
        listener: TcpListener,
        pool: WorkerPool<TcpStream>,
        shutdown: Arc<AtomicBool>,
    }

    #[derive(Clone)]
    pub struct ShutdownHandle {
        flag: Arc<AtomicBool>,
        addr: SocketAddr,
    }

    impl ShutdownHandle {
        pub fn shutdown(&self) {
            self.flag.store(true, Ordering::SeqCst);
            let _ = TcpStream::connect(self.addr); // wake the blocking accept()
        }
    }

    impl Server {
        pub fn bind<S>(addr: &str, store: Arc<S>, config: Config) -> io::Result<Server>
        where
            S: KvStore + Send + Sync + 'static,
        {
            let listener = TcpListener::bind(addr)?;
            let (workers, capacity) = (config.workers, config.queue_capacity);
            let pool = WorkerPool::new(workers, capacity, "ferrite", move |stream: TcpStream| {
                ACTIVE.fetch_add(1, Ordering::Relaxed);
                PEAK.fetch_max(ACTIVE.load(Ordering::Relaxed), Ordering::Relaxed);
                let _ = handle_connection(stream, &*store, &config); // I/O errors end the connection only
                ACTIVE.fetch_sub(1, Ordering::Relaxed);
            });
            Ok(Server { listener, pool, shutdown: Arc::new(AtomicBool::new(false)) })
        }

        pub fn local_addr(&self) -> SocketAddr {
            self.listener.local_addr().unwrap()
        }

        pub fn shutdown_handle(&self) -> ShutdownHandle {
            ShutdownHandle { flag: Arc::clone(&self.shutdown), addr: self.local_addr() }
        }

        /// Accepts until shutdown; returns the pool's (completed, panicked, rejected) counts.
        pub fn run(self) -> (u64, u64, u64) {
            for stream in self.listener.incoming() {
                if self.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                if let Err(mut stream) = self.pool.try_submit(stream) {
                    let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
                    let _ = stream.write_all(b"-ERR server busy\n");
                }
            }
            self.pool.shutdown()
        }
    }

    fn handle_connection<S: KvStore + ?Sized>(stream: TcpStream, store: &S, config: &Config) -> io::Result<()> {
        stream.set_read_timeout(Some(config.idle_timeout))?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut writer = BufWriter::new(stream);
        let mut line = Vec::with_capacity(256);
        loop {
            line.clear();
            let n = reader.by_ref().take(config.max_line as u64 + 1).read_until(b'\n', &mut line)?; // timeout = Err
            if n == 0 {
                return Ok(()); // the client closed the connection
            }
            if n > config.max_line {
                // We can't find the next line boundary cheaply: report and hang up.
                Response::Error("line too long".into()).write_to(&mut writer)?;
                return writer.flush();
            }
            while matches!(line.last(), Some(b'\n' | b'\r')) {
                line.pop();
            }
            let response = match std::str::from_utf8(&line) {
                Ok(text) => match protocol::parse(text) {
                    Ok(request) => protocol::execute(store, request),
                    Err(msg) => Response::Error(msg),
                },
                Err(_) => Response::Error("invalid UTF-8".into()),
            };
            response.write_to(&mut writer)?;
            if reader.buffer().is_empty() {
                writer.flush()?; // flush only when no pipelined request is already waiting: fewer syscalls
            }
        }
    }
}

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use store::{KvStore, ShardedStore};

/// A tiny blocking client: one connection, one request line at a time.
struct Client {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl Client {
    fn connect(addr: SocketAddr) -> Client {
        let s = TcpStream::connect(addr).unwrap();
        Client { writer: s.try_clone().unwrap(), reader: BufReader::new(s) }
    }

    fn send_raw(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap();
    }

    fn read_reply(&mut self) -> String {
        let mut reply = String::new();
        self.reader.read_line(&mut reply).unwrap();
        reply.trim_end_matches('\n').to_string()
    }

    fn call(&mut self, line: &str) -> String {
        self.send_raw(format!("{line}\n").as_bytes());
        self.read_reply()
    }
}

fn main() {
    let store = Arc::new(ShardedStore::new(16));
    let config = server::Config { workers: 4, queue_capacity: 16, idle_timeout: Duration::from_secs(2), max_line: 64 * 1024 };
    let srv = server::Server::bind("127.0.0.1:0", Arc::clone(&store), config).unwrap();
    let addr = srv.local_addr();
    let shutdown = srv.shutdown_handle();
    let server_thread = thread::spawn(move || srv.run());

    println!("-- one session");
    let mut c = Client::connect(addr);
    for cmd in [
        "PING", "SET user:1 ada", "GET user:1", "GET user:2", "SET user:1 grace", "get user:1", "DEL user:1",
        "DEL user:1", "GET user:1", "BOGUS 1", "GET", "SET k v extra", "",
    ] {
        println!("> {cmd:<18} < {}", c.call(cmd));
    }

    println!("-- pipelining: three requests in one write, three replies");
    c.send_raw(b"SET a 1\nSET b 2\nGET a\n");
    let replies: Vec<String> = (0..3).map(|_| c.read_reply()).collect();
    println!("{replies:?}");
    drop(c);

    println!("-- 8 concurrent clients x 1,000 commands, 4 workers");
    let t = Instant::now();
    let clients: Vec<_> = (0..8)
        .map(|id| {
            thread::spawn(move || {
                let mut c = Client::connect(addr);
                let mut correct = true;
                for i in 0..500 {
                    correct &= c.call(&format!("SET c{id}:k{i} v{i}")) == "+OK";
                }
                for i in 0..500 {
                    correct &= c.call(&format!("GET c{id}:k{i}")) == format!("$v{i}");
                }
                correct
            })
        })
        .collect();
    let all_correct = clients.into_iter().all(|h| h.join().unwrap());
    let secs = t.elapsed().as_secs_f64();
    println!("all 8,000 replies correct: {all_correct}; {:.0} commands/s over TCP (one run)", 8_000.0 / secs);
    println!("peak connections served at once: {} (= workers: one worker per connection)", server::PEAK.load(Ordering::Relaxed));

    println!("-- the library without the network: 8 threads x 100,000 ops on the same store");
    let t = Instant::now();
    thread::scope(|s| {
        for id in 0..8u32 {
            let store = &store;
            s.spawn(move || {
                for i in 0..100_000u32 {
                    let key = format!("lib{id}:{}", i % 1_000).into_bytes();
                    if i % 4 == 0 {
                        store.put(key, i.to_le_bytes().to_vec());
                    } else {
                        std::hint::black_box(store.get(&key));
                    }
                }
            });
        }
    });
    let secs = t.elapsed().as_secs_f64();
    println!("{:.1} M ops/s in-process (one run)", 800_000.0 / secs / 1e6);
    let lens = store.shard_lens();
    println!(
        "keys: {} across 16 shards (smallest {}, largest {}); poison recoveries: {}",
        store.len(),
        lens.iter().min().unwrap(),
        lens.iter().max().unwrap(),
        store.poison_recoveries()
    );

    shutdown.shutdown();
    let (completed, panicked, rejected) = server_thread.join().unwrap();
    println!("server stopped: {completed} connections served, {panicked} panics, {rejected} refused");
}

#[cfg(test)]
mod tests {
    use super::protocol::{execute, parse, Request, Response};
    use super::store::{KvStore, ShardedStore};
    use super::*;

    #[test]
    fn parse_accepts_the_v1_grammar() {
        assert_eq!(parse("PING"), Ok(Request::Ping));
        assert_eq!(parse("  get   k  "), Ok(Request::Get(b"k".to_vec())));
        assert_eq!(parse("SET k v"), Ok(Request::Set(b"k".to_vec(), b"v".to_vec())));
        assert_eq!(parse("del k"), Ok(Request::Del(b"k".to_vec())));
    }

    #[test]
    fn parse_rejects_bad_input_with_a_message() {
        assert_eq!(parse(""), Err("empty command".into()));
        assert_eq!(parse("GET"), Err("wrong number of arguments for 'GET'".into()));
        assert_eq!(parse("set k"), Err("wrong number of arguments for 'SET'".into()));
        assert_eq!(parse("SET k v x"), Err("wrong number of arguments for 'SET'".into()));
        assert_eq!(parse("PING x"), Err("wrong number of arguments for 'PING'".into()));
        assert_eq!(parse("FLUSHALL"), Err("unknown command 'FLUSHALL'".into()));
    }

    #[test]
    fn responses_encode_as_specified() {
        let enc = |r: Response| {
            let mut out = Vec::new();
            r.write_to(&mut out).unwrap();
            String::from_utf8(out).unwrap()
        };
        assert_eq!(enc(Response::Pong), "+PONG\n");
        assert_eq!(enc(Response::Ok), "+OK\n");
        assert_eq!(enc(Response::Value(b"42".to_vec())), "$42\n");
        assert_eq!(enc(Response::Nil), "_\n");
        assert_eq!(enc(Response::Error("boom".into())), "-ERR boom\n");
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

    #[test]
    fn works_as_a_trait_object() {
        let s: Box<dyn KvStore + Send + Sync> = Box::new(ShardedStore::new(2));
        assert_eq!(execute(&*s, Request::Set(b"a".to_vec(), b"1".to_vec())), Response::Ok);
        assert_eq!(execute(&*s, Request::Get(b"a".to_vec())), Response::Value(b"1".to_vec()));
        assert_eq!(execute(&*s, Request::Del(b"zz".to_vec())), Response::Nil);
    }

    #[test]
    fn keys_spread_across_shards() {
        let s = ShardedStore::new(16);
        for i in 0..16_000 {
            s.put(format!("key:{i}").into_bytes(), vec![]);
        }
        let lens = s.shard_lens();
        assert!(lens.iter().all(|&n| (750..=1250).contains(&n)), "uneven shards: {lens:?}");
    }

    #[test]
    fn a_poisoned_shard_is_recovered_once() {
        std::panic::set_hook(Box::new(|_| {}));
        let s = ShardedStore::new(4);
        s.put(b"k".to_vec(), b"v".to_vec());
        s.poison_shard_of(b"k");
        assert_eq!(s.get(b"k"), Some(b"v".to_vec())); // recovered: the data is still served
        assert_eq!(s.get(b"k"), Some(b"v".to_vec()));
        assert_eq!(s.poison_recoveries(), 1); // counted once: the flag was cleared
    }

    #[test]
    fn end_to_end_over_tcp() {
        let store = Arc::new(ShardedStore::new(4));
        let config = server::Config { workers: 2, queue_capacity: 4, idle_timeout: Duration::from_secs(2), max_line: 32 };
        let srv = server::Server::bind("127.0.0.1:0", Arc::clone(&store), config).unwrap();
        let (addr, shutdown) = (srv.local_addr(), srv.shutdown_handle());
        let t = thread::spawn(move || srv.run());
        let mut c = Client::connect(addr);
        assert_eq!(c.call("SET x 1"), "+OK");
        assert_eq!(c.call("GET x"), "$1");
        assert_eq!(c.call(&"A".repeat(40)), "-ERR line too long");
        drop(c);
        assert_eq!(store.get(b"x"), Some(b"1".to_vec()));
        shutdown.shutdown();
        let (_, panicked, _) = t.join().unwrap();
        assert_eq!(panicked, 0);
    }
}
