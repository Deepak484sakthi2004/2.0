// verify: debug ok
// verify: debug test
// verify: release ok
//! minihttp: an HTTP/1.1 server on raw std::net TCP, with a worker pool built here. No dependencies.
//!
//! - pool:   a fixed set of worker threads fed by a BOUNDED queue of *items* (here: connections) through
//!           Mutex + Condvar. try_submit hands a refused item back; workers survive a panicking handler;
//!           Drop = graceful shutdown (stop accepting, drain the queue, join every worker).
//! - http:   request parsing with hard limits (line length, header count and bytes, body size).
//! - app:    a few routes, including one that panics and one that is slow (for the demonstrations).
//! - server: accept loop, 503 when the queue is full, keep-alive with a per-connection request cap,
//!           read/write timeouts, a shutdown handle.
//!
//! `main` starts the server on 127.0.0.1 and exercises it with in-process clients.

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

mod http {
    use std::io::{self, BufRead, Read, Write};

    pub struct Limits {
        pub max_line: usize,
        pub max_headers: usize,
        pub max_header_bytes: usize,
        pub max_body: usize,
    }

    impl Default for Limits {
        fn default() -> Limits {
            Limits { max_line: 8 * 1024, max_headers: 64, max_header_bytes: 16 * 1024, max_body: 1024 * 1024 }
        }
    }

    #[derive(Debug, PartialEq, Clone, Copy)]
    pub enum Version {
        Http10,
        Http11,
    }

    #[derive(Debug)]
    pub struct Request {
        pub method: String,
        pub target: String,
        pub version: Version,
        pub headers: Vec<(String, String)>,
        pub body: Vec<u8>,
    }

    impl Request {
        pub fn header(&self, name: &str) -> Option<&str> {
            self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
        }

        /// HTTP/1.1 connections persist unless the client says `close`; HTTP/1.0 ones close unless it asks.
        pub fn keep_alive(&self) -> bool {
            let conn = self.header("connection").map(|v| v.to_ascii_lowercase());
            match self.version {
                Version::Http11 => conn.as_deref() != Some("close"),
                Version::Http10 => conn.as_deref() == Some("keep-alive"),
            }
        }

        pub fn path_and_query(&self) -> (&str, Option<&str>) {
            match self.target.split_once('?') {
                Some((p, q)) => (p, Some(q)),
                None => (self.target.as_str(), None),
            }
        }
    }

    #[derive(Debug)]
    pub enum ParseError {
        Closed,                 // the client closed the connection before sending a request: not an error
        Io(io::Error),          // includes the idle/read timeout (WouldBlock or TimedOut)
        BadRequest(&'static str),
        HeadersTooLarge,        // 431
        BodyTooLarge,           // 413
        VersionNotSupported,    // 505
        NotImplemented(&'static str), // 501
    }

    impl ParseError {
        /// The response to send before closing, if any.
        pub fn response(&self) -> Option<Response> {
            Some(match self {
                ParseError::Closed | ParseError::Io(_) => return None,
                ParseError::BadRequest(why) => Response::text(400, "Bad Request", why),
                ParseError::HeadersTooLarge => Response::text(431, "Request Header Fields Too Large", "headers too large"),
                ParseError::BodyTooLarge => Response::text(413, "Content Too Large", "body too large"),
                ParseError::VersionNotSupported => Response::text(505, "HTTP Version Not Supported", "HTTP/1.x only"),
                ParseError::NotImplemented(what) => Response::text(501, "Not Implemented", what),
            })
        }
    }

    /// Reads one line (up to and including b'\n') but never more than `limit` bytes.
    fn read_line(r: &mut impl BufRead, limit: usize, buf: &mut Vec<u8>) -> Result<usize, ParseError> {
        buf.clear();
        let n = r.by_ref().take(limit as u64 + 1).read_until(b'\n', buf).map_err(ParseError::Io)?;
        if n > limit {
            return Err(ParseError::HeadersTooLarge);
        }
        if n > 0 && buf.last() != Some(&b'\n') {
            return Err(ParseError::BadRequest("unexpected end of request"));
        }
        while matches!(buf.last(), Some(b'\n' | b'\r')) {
            buf.pop();
        }
        Ok(n)
    }

    pub fn read_request(r: &mut impl BufRead, limits: &Limits) -> Result<Request, ParseError> {
        let mut line = Vec::with_capacity(256);
        if read_line(r, limits.max_line, &mut line)? == 0 {
            return Err(ParseError::Closed);
        }
        let text = std::str::from_utf8(&line).map_err(|_| ParseError::BadRequest("request line is not UTF-8"))?;
        let mut parts = text.split(' ');
        let (Some(method), Some(target), Some(version), None) = (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(ParseError::BadRequest("malformed request line"));
        };
        let version = match version {
            "HTTP/1.1" => Version::Http11,
            "HTTP/1.0" => Version::Http10,
            v if v.starts_with("HTTP/") => return Err(ParseError::VersionNotSupported),
            _ => return Err(ParseError::BadRequest("malformed request line")),
        };
        if method.is_empty() || !target.starts_with('/') {
            return Err(ParseError::BadRequest("malformed request line"));
        }
        // `method` and `target` borrow `line`, which the header loop reuses: copy them out first (E0502 otherwise).
        let (method, target) = (method.to_string(), target.to_string());

        let mut headers = Vec::new();
        let mut header_bytes = 0;
        loop {
            let n = read_line(r, limits.max_line, &mut line)?;
            if n == 0 {
                return Err(ParseError::BadRequest("unexpected end of headers"));
            }
            if line.is_empty() {
                break; // the blank line ends the header section
            }
            header_bytes += n;
            if headers.len() == limits.max_headers || header_bytes > limits.max_header_bytes {
                return Err(ParseError::HeadersTooLarge);
            }
            let text = std::str::from_utf8(&line).map_err(|_| ParseError::BadRequest("header is not UTF-8"))?;
            let (name, value) = text.split_once(':').ok_or(ParseError::BadRequest("header without ':'"))?;
            if name.is_empty() || name.contains(char::is_whitespace) {
                return Err(ParseError::BadRequest("invalid header name"));
            }
            headers.push((name.to_string(), value.trim().to_string()));
        }

        let mut req = Request { method, target, version, headers, body: Vec::new() };
        if req.header("transfer-encoding").is_some() {
            return Err(ParseError::NotImplemented("chunked request bodies are not supported"));
        }
        if let Some(len) = req.header("content-length") {
            let len: usize = len.parse().map_err(|_| ParseError::BadRequest("invalid Content-Length"))?;
            if len > limits.max_body {
                return Err(ParseError::BodyTooLarge);
            }
            req.body.resize(len, 0);
            r.read_exact(&mut req.body).map_err(ParseError::Io)?;
        }
        Ok(req)
    }

    #[derive(Debug)]
    pub struct Response {
        pub status: u16,
        pub reason: &'static str,
        pub body: Vec<u8>,
    }

    impl Response {
        pub fn text(status: u16, reason: &'static str, body: &str) -> Response {
            Response { status, reason, body: format!("{body}\n").into_bytes() }
        }

        /// One write_all per response: head and body assembled in a single buffer.
        pub fn write_to(&self, w: &mut impl Write, keep_alive: bool) -> io::Result<()> {
            let mut out = Vec::with_capacity(128 + self.body.len());
            write!(
                out,
                "HTTP/1.1 {} {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: {}\r\n\r\n",
                self.status,
                self.reason,
                self.body.len(),
                if keep_alive { "keep-alive" } else { "close" }
            )?;
            out.extend_from_slice(&self.body);
            w.write_all(&out)?;
            w.flush()
        }
    }
}

mod app {
    use crate::http::{Request, Response};
    use std::time::Duration;

    pub fn route(req: &Request) -> Response {
        let (path, query) = req.path_and_query();
        let param = |name: &str| {
            query.and_then(|q| q.split('&').filter_map(|kv| kv.split_once('=')).find(|(k, _)| *k == name).map(|(_, v)| v))
        };
        match (req.method.as_str(), path) {
            ("GET", "/health") => Response::text(200, "OK", "ok"),
            ("GET", "/hello") => Response::text(200, "OK", &format!("hello, {}", param("name").unwrap_or("world"))),
            ("POST", "/echo") => Response { status: 200, reason: "OK", body: req.body.clone() },
            ("GET", "/slow") => {
                let ms = param("ms").and_then(|v| v.parse().ok()).unwrap_or(100u64).min(5_000);
                std::thread::sleep(Duration::from_millis(ms));
                Response::text(200, "OK", &format!("slept {ms} ms"))
            }
            ("GET", "/panic") => panic!("handler bug: index out of range in report builder"),
            (_, "/health" | "/hello" | "/echo" | "/slow" | "/panic") => Response::text(405, "Method Not Allowed", "method not allowed"),
            _ => Response::text(404, "Not Found", "not found"),
        }
    }
}


mod server {
    use crate::http::{self, Limits, ParseError, Response};
    use crate::pool::WorkerPool;
    use std::io::{self, BufReader};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::panic::{self, AssertUnwindSafe};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// Connections closed because the client sent nothing within the read timeout.
    pub static IDLE_TIMEOUTS: AtomicU64 = AtomicU64::new(0);

    pub struct Config {
        pub workers: usize,
        pub queue_capacity: usize,
        pub read_timeout: Duration,
        pub write_timeout: Duration,
        pub max_requests_per_connection: usize,
    }

    pub struct Server {
        listener: TcpListener,
        pool: WorkerPool<TcpStream>,
        config: Arc<Config>,
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
            let _ = TcpStream::connect(self.addr); // wake the blocking accept() so it sees the flag
        }
    }

    impl Server {
        pub fn bind(addr: &str, config: Config) -> io::Result<Server> {
            let listener = TcpListener::bind(addr)?;
            let config = Arc::new(config);
            let for_workers = Arc::clone(&config);
            let pool = WorkerPool::new(config.workers, config.queue_capacity, "http", move |stream| {
                handle_connection(stream, &for_workers)
            });
            Ok(Server { listener, pool, config, shutdown: Arc::new(AtomicBool::new(false)) })
        }

        pub fn local_addr(&self) -> SocketAddr {
            self.listener.local_addr().unwrap()
        }

        pub fn shutdown_handle(&self) -> ShutdownHandle {
            ShutdownHandle { flag: Arc::clone(&self.shutdown), addr: self.local_addr() }
        }

        /// Runs until shutdown; returns (completed, panicked, rejected) connection counts.
        pub fn run(self) -> (u64, u64, u64) {
            for stream in self.listener.incoming() {
                if self.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else {
                    continue; // e.g. EMFILE, or a connection reset before accept(): keep serving
                };
                let _ = stream.set_read_timeout(Some(self.config.read_timeout));
                let _ = stream.set_write_timeout(Some(self.config.write_timeout));
                if let Err(stream) = self.pool.try_submit(stream) {
                    refuse(stream); // the queue is full: answer 503 now instead of queueing forever
                }
            }
            self.pool.shutdown() // graceful: queued connections are still served, then workers are joined
        }
    }

    fn handle_connection(stream: TcpStream, config: &Config) {
        let limits = Limits::default();
        let Ok(read_half) = stream.try_clone() else { return };
        let mut reader = BufReader::new(read_half);
        let mut writer = stream;
        for served in 1..=config.max_requests_per_connection {
            let req = match http::read_request(&mut reader, &limits) {
                Ok(req) => req,
                Err(e) => {
                    // Closed or Io (idle timeout, reset): nothing to say. Protocol errors get a response.
                    if let ParseError::Io(err) = &e {
                        if matches!(err.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) {
                            IDLE_TIMEOUTS.fetch_add(1, Ordering::Relaxed); // the read timeout fired
                        }
                    }
                    if let Some(resp) = e.response() {
                        let _ = resp.write_to(&mut writer, false);
                    }
                    return;
                }
            };
            // A handler bug becomes a 500 for this request; the connection and the worker carry on.
            let resp = panic::catch_unwind(AssertUnwindSafe(|| crate::app::route(&req)))
                .unwrap_or_else(|_| Response::text(500, "Internal Server Error", "internal error"));
            let keep_alive = req.keep_alive() && served < config.max_requests_per_connection;
            if resp.write_to(&mut writer, keep_alive).is_err() || !keep_alive {
                return;
            }
        }
    }

    /// Runs on the accept thread: must never block for long.
    fn refuse(mut stream: TcpStream) {
        let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
        let _ = Response::text(503, "Service Unavailable", "server busy").write_to(&mut stream, false);
    }
}

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

/// Sends raw bytes, reads until the server closes, returns the whole response.
fn roundtrip(addr: SocketAddr, raw: &[u8]) -> String {
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(raw).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

fn status_and_body(resp: &str) -> String {
    let status = resp.lines().next().unwrap_or("<no response>").to_string();
    let body = resp.split("\r\n\r\n").nth(1).unwrap_or("").trim_end();
    if body.is_empty() { status } else { format!("{status} | {body}") }
}

/// Reads one response off a persistent connection (headers, then a Content-Length body).
fn read_one(r: &mut BufReader<TcpStream>) -> String {
    let (mut status, mut len, mut line) = (String::new(), 0usize, String::new());
    r.read_line(&mut status).unwrap();
    loop {
        line.clear();
        r.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse().unwrap();
        }
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body).unwrap();
    format!("{} | {}", status.trim_end(), String::from_utf8_lossy(&body).trim_end())
}

fn main() {
    let config = server::Config {
        workers: 4,
        queue_capacity: 2,
        read_timeout: Duration::from_millis(500),
        write_timeout: Duration::from_millis(500),
        max_requests_per_connection: 100,
    };
    let srv = server::Server::bind("127.0.0.1:0", config).unwrap();
    let addr = srv.local_addr();
    let shutdown = srv.shutdown_handle();
    let server_thread = thread::spawn(move || srv.run());

    println!("-- basic requests (Connection: close)");
    for raw in [
        "GET /health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        "GET /hello?name=Ada HTTP/1.1\r\nConnection: close\r\n\r\n",
        "POST /echo HTTP/1.1\r\nContent-Length: 11\r\nConnection: close\r\n\r\npayment=42\n",
        "DELETE /health HTTP/1.1\r\nConnection: close\r\n\r\n",
        "GET /nope HTTP/1.1\r\nConnection: close\r\n\r\n",
    ] {
        println!("{}", status_and_body(&roundtrip(addr, raw.as_bytes())));
    }

    println!("-- a handler panics; the server keeps serving");
    std::panic::set_hook(Box::new(|info| {
        eprintln!("[panic hook] {}", info.payload_as_str().unwrap_or("?")); // logs go to stderr
    }));
    println!("{}", status_and_body(&roundtrip(addr, b"GET /panic HTTP/1.1\r\nConnection: close\r\n\r\n")));
    println!("{}", status_and_body(&roundtrip(addr, b"GET /health HTTP/1.1\r\nConnection: close\r\n\r\n")));

    println!("-- limits and protocol errors");
    let many_headers: String = (0..70).map(|i| format!("X-H{i}: v\r\n")).collect();
    for raw in [
        format!("GET /health HTTP/1.1\r\n{many_headers}\r\n"),
        "POST /echo HTTP/1.1\r\nContent-Length: 99999999\r\n\r\n".to_string(),
        "GET /health HTTP/2.0\r\n\r\n".to_string(),
        "HELLO\r\n\r\n".to_string(),
        "POST /echo HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n".to_string(),
    ] {
        println!("{}", status_and_body(&roundtrip(addr, raw.as_bytes())));
    }

    println!("-- keep-alive: two requests, one connection");
    {
        let s = TcpStream::connect(addr).unwrap();
        let mut w = s.try_clone().unwrap();
        let mut r = BufReader::new(s);
        w.write_all(b"GET /hello?name=first HTTP/1.1\r\n\r\n").unwrap();
        println!("{}", read_one(&mut r));
        w.write_all(b"GET /hello?name=second HTTP/1.1\r\nConnection: close\r\n\r\n").unwrap();
        println!("{}", read_one(&mut r));
    }

    println!("-- an idle connection holds a worker until the read timeout");
    {
        let t = Instant::now();
        let mut s = TcpStream::connect(addr).unwrap();
        let mut buf = [0u8; 1];
        let n = s.read(&mut buf).unwrap_or(0); // we send nothing; the server gives up and closes
        println!("server closed the idle connection (read returned {n}) after ~500 ms: {}", t.elapsed() >= Duration::from_millis(450));
    }

    println!("-- overload: 9 slow requests against 4 workers + a queue of 2");
    {
        let clients: Vec<_> = (0..9)
            .map(|_| {
                thread::sleep(Duration::from_millis(20)); // arrive one after another
                thread::spawn(move || roundtrip(addr, b"GET /slow?ms=400 HTTP/1.1\r\nConnection: close\r\n\r\n"))
            })
            .collect();
        let statuses: Vec<String> = clients.into_iter().map(|c| c.join().unwrap().lines().next().unwrap_or("").to_string()).collect();
        let ok = statuses.iter().filter(|s| s.contains(" 200 ")).count();
        let busy = statuses.iter().filter(|s| s.contains(" 503 ")).count();
        println!("200 OK: {ok}, 503 Service Unavailable: {busy}");
    }

    println!("-- graceful shutdown: an in-flight request still completes");
    let inflight = thread::spawn(move || roundtrip(addr, b"GET /slow?ms=300 HTTP/1.1\r\nConnection: close\r\n\r\n"));
    thread::sleep(Duration::from_millis(100));
    shutdown.shutdown();
    let (completed, panicked, rejected) = server_thread.join().unwrap();
    println!("in-flight request: {}", status_and_body(&inflight.join().unwrap()));
    println!("pool: {completed} connections handled, {panicked} handler panics escaped, {rejected} refused");
    println!("idle connections closed by the read timeout: {}", server::IDLE_TIMEOUTS.load(std::sync::atomic::Ordering::Relaxed));
}

#[cfg(test)]
mod tests {
    use super::http::{read_request, Limits, ParseError, Request, Version};
    use super::pool::WorkerPool;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    fn parse(raw: &str) -> Result<Request, ParseError> {
        read_request(&mut Cursor::new(raw.as_bytes().to_vec()), &Limits::default())
    }

    #[test]
    fn parses_request_line_headers_and_body() {
        let req = parse("POST /echo?x=1 HTTP/1.1\r\nHost: a\r\ncontent-LENGTH: 3\r\n\r\nabc").unwrap();
        assert_eq!((req.method.as_str(), req.target.as_str(), req.version), ("POST", "/echo?x=1", Version::Http11));
        assert_eq!(req.header("Content-Length"), Some("3"));
        assert_eq!(req.body, b"abc");
        assert_eq!(req.path_and_query(), ("/echo", Some("x=1")));
    }

    #[test]
    fn keep_alive_rules() {
        assert!(parse("GET / HTTP/1.1\r\n\r\n").unwrap().keep_alive());
        assert!(!parse("GET / HTTP/1.1\r\nConnection: close\r\n\r\n").unwrap().keep_alive());
        assert!(!parse("GET / HTTP/1.0\r\n\r\n").unwrap().keep_alive());
        assert!(parse("GET / HTTP/1.0\r\nConnection: Keep-Alive\r\n\r\n").unwrap().keep_alive());
    }

    #[test]
    fn limits_and_errors() {
        let many: String = (0..65).map(|i| format!("h{i}: v\r\n")).collect();
        assert!(matches!(parse(&format!("GET / HTTP/1.1\r\n{many}\r\n")), Err(ParseError::HeadersTooLarge)));
        let long = format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(9_000));
        assert!(matches!(parse(&long), Err(ParseError::HeadersTooLarge)));
        assert!(matches!(parse("POST / HTTP/1.1\r\nContent-Length: 2000000\r\n\r\n"), Err(ParseError::BodyTooLarge)));
        assert!(matches!(parse("GET / HTTP/3\r\n\r\n"), Err(ParseError::VersionNotSupported)));
        assert!(matches!(parse("GET /\r\n\r\n"), Err(ParseError::BadRequest(_))));
        assert!(matches!(parse("GET / HTTP/1.1\r\nno-colon\r\n\r\n"), Err(ParseError::BadRequest(_))));
        assert!(matches!(parse("GET / HTTP/1.1\r\nHost: a\r\n"), Err(ParseError::BadRequest(_))));
        assert!(matches!(parse(""), Err(ParseError::Closed)));
        assert!(matches!(parse("POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n"), Err(ParseError::NotImplemented(_))));
    }

    #[test]
    fn workers_survive_panics_and_drop_drains_the_queue() {
        std::panic::set_hook(Box::new(|_| {})); // keep the test output quiet
        let ran = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&ran);
        let pool = WorkerPool::new(2, 16, "t", move |i: u32| {
            if i % 3 == 0 {
                panic!("item {i} failed");
            }
            counter.fetch_add(1, Ordering::Relaxed);
        });
        for i in 0..10 {
            pool.try_submit(i).unwrap();
        }
        drop(pool); // joins the workers after every queued item has been handled
        assert_eq!(ran.load(Ordering::Relaxed), 6); // 1, 2, 4, 5, 7, 8
    }

    #[test]
    fn a_full_queue_hands_the_item_back() {
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let release_rx = std::sync::Mutex::new(release_rx);
        let pool = WorkerPool::new(1, 1, "t", move |i: u32| {
            if i == 0 {
                let _ = release_rx.lock().unwrap().recv(); // item 0 occupies the only worker
            }
        });
        pool.try_submit(0).unwrap();
        std::thread::sleep(Duration::from_millis(50)); // let the worker take item 0
        pool.try_submit(1).unwrap(); // fills the single queue slot
        assert_eq!(pool.try_submit(2), Err(2)); // refused, and we get it back
        release_tx.send(()).unwrap();
        let (completed, panicked, rejected) = pool.shutdown();
        assert_eq!((completed, panicked, rejected), (2, 0, 1));
    }
}
