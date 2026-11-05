//! Sending files (client) and receiving them (listener, pairing mode). No GUI imports.
use crate::config::{self, Peer};
use crate::pair;
use crate::proto::{Msg, Session, CHUNK};
use crate::store;
use std::io::Read;
use std::net::{IpAddr, Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const IO_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// How long a pairing code stays valid after it was set.
pub const PAIR_WINDOW: Duration = Duration::from_secs(120);

#[derive(Debug)]
pub struct Received {
    pub from: String,
    pub files: Vec<PathBuf>,
}

#[derive(Debug)]
pub enum Event {
    Received(Received),
    Paired(String),
    Refused(IpAddr),
    Error(String),
}

pub struct Listener {
    sock: TcpListener,
    /// Code and the moment it was set; `Some` only while this PC waits to be paired.
    pub pairing: Arc<Mutex<Option<(String, Instant)>>>,
    downloads: PathBuf,
    config_dir: PathBuf,
}

fn trunc(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

fn timeouts(s: &TcpStream) {
    let _ = s.set_read_timeout(Some(IO_TIMEOUT));
    let _ = s.set_write_timeout(Some(IO_TIMEOUT));
}

/// Resolve and connect (3 s), with the 30 s I/O timeouts set.
fn connect(host: &str) -> Result<TcpStream, String> {
    let addr = config::resolve(host)?;
    let s = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|_| "other PC unreachable".to_string())?;
    timeouts(&s);
    Ok(s)
}

/// Pair with the PC showing `code`, and remember it.
pub fn pair_with(host: &str, code: &str) -> Result<(), String> {
    let code = pair::parse_code(code)?;
    let mut s = connect(host)?;
    let secret = pair::join(&mut s, &code)?;
    config::save_peer(&Peer { host: host.to_string(), secret })
}

/// Every path must be an existing regular file. Runs before connecting.
pub fn check_files(paths: &[PathBuf]) -> Result<(), String> {
    for p in paths {
        let name = p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned());
        match std::fs::metadata(p) {
            Err(_) => return Err(format!("not sent: {} not found", p.display())),
            Ok(m) if m.is_dir() => return Err(format!("not sent: {name} is a folder, zip it first")),
            Ok(m) if !m.is_file() => return Err(format!("not sent: {name} is not a regular file")),
            Ok(_) => {}
        }
    }
    Ok(())
}

pub fn send_files(peer: &Peer, paths: &[PathBuf]) -> Result<String, String> {
    let count = u16::try_from(paths.len()).map_err(|_| "not sent: too many files at once".to_string())?;
    let stream = connect(&peer.host).map_err(|e| format!("not sent: {e}"))?;
    let mut sess = Session::client(stream, &peer.secret)
        .map_err(|_| "not sent: the other PC refused (pairing doesn't match: pair again)".to_string())?;
    if let Err(e) = send_batch(&mut sess, count, paths) {
        // The receiver may have said why before it hung up.
        return Err(match sess.recv() {
            Ok(Msg::Fail(r)) => format!("not sent: {r}"),
            _ => format!("not sent: {e}"),
        });
    }
    match sess.recv() {
        Ok(Msg::Done { saved }) if saved == count => {
            Ok(format!("{count} file{} sent to {}", if count == 1 { "" } else { "s" }, peer.host))
        }
        Ok(Msg::Fail(r)) => Err(format!("not sent: {r}")),
        Ok(_) => Err("not sent: the other PC did not confirm".into()),
        Err(e) => Err(format!("not sent: {e}")),
    }
}

fn send_batch(sess: &mut Session<TcpStream>, count: u16, paths: &[PathBuf]) -> Result<(), String> {
    sess.send(&Msg::Batch { count, from: trunc(&config::this_pc_name(), 200) })?;
    for p in paths {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let f = std::fs::File::open(p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
        let size = f.metadata().map_err(|e| format!("cannot read {}: {e}", p.display()))?.len();
        sess.send(&Msg::File { size, name: trunc(&name, 200) })?;
        let (mut f, mut buf, mut sent) = (f.take(size), vec![0u8; CHUNK], 0u64);
        loop {
            let n = f.read(&mut buf).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
            if n == 0 {
                break;
            }
            sess.send(&Msg::Data(buf[..n].to_vec()))?;
            sent += n as u64;
        }
        if sent != size {
            return Err(format!("{name} changed while sending"));
        }
        sess.send(&Msg::End)?;
    }
    Ok(())
}

/// One batch of files from a connected, mode-byte-consumed stream into `dir`.
pub fn serve_one(stream: TcpStream, secret: &[u8; 32], dir: &Path) -> Result<Received, String> {
    timeouts(&stream);
    let keep = stream.try_clone().map_err(|e| e.to_string())?;
    let mut sess = Session::server(stream, secret)?;
    match receive(&mut sess, dir) {
        Ok(r) => {
            sess.send(&Msg::Done { saved: r.files.len() as u16 })?;
            Ok(r)
        }
        Err(e) => {
            let _ = sess.send(&Msg::Fail(trunc(&e, 1000)));
            // Let the sender finish writing and read the Fail; closing with unread data would reset it.
            let _ = keep.shutdown(Shutdown::Write);
            let _ = keep.set_read_timeout(Some(Duration::from_secs(5)));
            let _ = std::io::copy(&mut &keep, &mut std::io::sink());
            Err(e)
        }
    }
}

fn receive(sess: &mut Session<TcpStream>, dir: &Path) -> Result<Received, String> {
    let Msg::Batch { count, from } = sess.recv()? else { return Err("unexpected message".into()) };
    let mut files = Vec::new();
    for _ in 0..count {
        let Msg::File { size, name } = sess.recv()? else { return Err("unexpected message".into()) };
        let name = store::sanitize_name(&name).ok_or("the file name is not usable")?;
        let mut inc = store::begin(dir, &name)?;
        loop {
            match sess.recv()? {
                Msg::Data(d) => inc.write(&d)?,
                Msg::End => break,
                _ => return Err("unexpected message".into()),
            }
        }
        files.push(inc.finish(size)?);
    }
    Ok(Received { from, files })
}

impl Listener {
    pub fn bind(port: u16, downloads: PathBuf) -> Result<Self, String> {
        Self::bind_with(port, downloads, config::config_dir()?)
    }

    pub fn bind_with(port: u16, downloads: PathBuf, config_dir: PathBuf) -> Result<Self, String> {
        let sock = TcpListener::bind(("0.0.0.0", port)).map_err(|e| match e.kind() {
            std::io::ErrorKind::AddrInUse => format!(
                "port {port} is busy: synergy-dragndrop is probably already running (use its tray menu \"Pair…\", or quit it first)"
            ),
            _ => format!("cannot listen on port {port}: {e}"),
        })?;
        Ok(Listener { sock, pairing: Arc::new(Mutex::new(None)), downloads, config_dir })
    }

    #[cfg(test)]
    pub fn port(&self) -> u16 {
        self.sock.local_addr().map_or(0, |a| a.port())
    }

    /// Accept forever, one thread per connection.
    pub fn run(&self, on_event: impl Fn(Event) + Send + Sync) -> ! {
        std::thread::scope(|scope| {
            let on_event = &on_event;
            loop {
                match self.sock.accept() {
                    Ok((s, addr)) => {
                        scope.spawn(move || self.handle(s, addr.ip(), on_event));
                    }
                    Err(e) => {
                        on_event(Event::Error(format!("accept failed: {e}")));
                        std::thread::sleep(Duration::from_millis(200));
                    }
                }
            }
        });
        unreachable!()
    }

    fn handle(&self, mut s: TcpStream, ip: IpAddr, on_event: &impl Fn(Event)) {
        timeouts(&s);
        let mut mode = [0u8; 1];
        if s.read_exact(&mut mode).is_err() {
            return;
        }
        match mode[0] {
            b'P' => {
                // One attempt per code, whatever the outcome.
                let armed = self.pairing.lock().unwrap_or_else(|e| e.into_inner()).take();
                let Some((code, _)) = armed.filter(|(_, t)| t.elapsed() < PAIR_WINDOW) else {
                    return on_event(Event::Refused(ip));
                };
                let saved = pair::host(&mut s, &code).and_then(|secret| {
                    config::save_peer_to(&self.config_dir, &Peer { host: ip.to_string(), secret })
                });
                on_event(match saved {
                    Ok(()) => Event::Paired(ip.to_string()),
                    Err(e) => Event::Error(e),
                });
            }
            b'S' => {
                let peer = match config::load_peer_from(&self.config_dir) {
                    Ok(p) => p,
                    Err(e) => return on_event(Event::Error(e)),
                };
                on_event(match serve_one(s, &peer.secret, &self.downloads) {
                    Ok(r) => Event::Received(r),
                    Err(e) if e.starts_with("handshake failed") => Event::Refused(ip),
                    Err(e) => Event::Error(e),
                });
            }
            _ => on_event(Event::Refused(ip)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let mut r = [0u8; 8];
        getrandom::fill(&mut r).unwrap();
        let d = std::env::temp_dir().join(format!("sdd-test-{}", config::hex(&r)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// One-shot receiver on loopback; the handle yields serve_one's result.
    fn serve(secret: [u8; 32], dir: PathBuf) -> (Peer, std::thread::JoinHandle<Result<Received, String>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let host = format!("127.0.0.1:{}", l.local_addr().unwrap().port());
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut mode = [0u8; 1];
            s.read_exact(&mut mode).unwrap();
            serve_one(s, &secret, &dir)
        });
        (Peer { host, secret: [0; 32] }, h)
    }

    fn peer_for(p: Peer, secret: [u8; 32]) -> Peer {
        Peer { secret, ..p }
    }

    fn count(d: &Path) -> usize {
        std::fs::read_dir(d).unwrap().count()
    }

    #[test]
    fn sends_two_files() {
        let (src, dst) = (tmp(), tmp());
        let big: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(src.join("a.txt"), b"hello").unwrap();
        std::fs::write(src.join("b.bin"), &big).unwrap();
        let (p, h) = serve([5; 32], dst.clone());
        let msg = send_files(&peer_for(p, [5; 32]), &[src.join("a.txt"), src.join("b.bin")]).unwrap();
        assert!(msg.contains("2 files sent"), "{msg}");
        let r = h.join().unwrap().unwrap();
        assert_eq!(r.files.len(), 2);
        assert_eq!(std::fs::read(dst.join("a.txt")).unwrap(), b"hello");
        assert_eq!(std::fs::read(dst.join("b.bin")).unwrap(), big);
        assert_eq!(count(&dst), 2);
    }

    #[test]
    fn folder_refused_before_connecting() {
        let d = tmp().join("stuff");
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(check_files(&[d]).unwrap_err(), "not sent: stuff is a folder, zip it first");
        assert!(check_files(&[tmp().join("nope")]).unwrap_err().ends_with("nope not found"));
    }

    #[test]
    fn unreachable_fails_fast() {
        let f = tmp().join("x");
        std::fs::write(&f, b"x").unwrap();
        let t = Instant::now();
        let e = send_files(&Peer { host: "127.0.0.1:1".into(), secret: [0; 32] }, &[f]).unwrap_err();
        assert_eq!(e, "not sent: other PC unreachable");
        assert!(t.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn wrong_secret_refused() {
        let (src, dst) = (tmp(), tmp());
        std::fs::write(src.join("a"), b"a").unwrap();
        let (p, h) = serve([1; 32], dst.clone());
        let e = send_files(&peer_for(p, [2; 32]), &[src.join("a")]).unwrap_err();
        assert!(e.contains("refused"), "{e}");
        assert!(h.join().unwrap().is_err());
        assert_eq!(count(&dst), 0);
    }

    #[test]
    fn truncated_stream_leaves_nothing() {
        let dst = tmp();
        let (p, h) = serve([3; 32], dst.clone());
        let mut s = Session::client(TcpStream::connect(&p.host).unwrap(), &[3; 32]).unwrap();
        s.send(&Msg::Batch { count: 1, from: "pc".into() }).unwrap();
        s.send(&Msg::File { size: 1000, name: "t.bin".into() }).unwrap();
        s.send(&Msg::Data(vec![7; 10])).unwrap();
        drop(s);
        assert!(h.join().unwrap().is_err());
        assert_eq!(count(&dst), 0);
    }

    #[test]
    fn receiver_error_reaches_sender() {
        let src = tmp();
        std::fs::write(src.join("a"), b"a").unwrap();
        let (p, h) = serve([4; 32], tmp().join("missing"));
        let e = send_files(&peer_for(p, [4; 32]), &[src.join("a")]).unwrap_err();
        assert!(e.starts_with("not sent: ") && !e.contains("refused"), "{e}");
        assert!(h.join().unwrap().is_err());
    }

    #[test]
    fn hostile_name_stays_in_dir() {
        let dst = tmp();
        let (p, h) = serve([6; 32], dst.clone());
        let mut s = Session::client(TcpStream::connect(&p.host).unwrap(), &[6; 32]).unwrap();
        s.send(&Msg::Batch { count: 1, from: "pc".into() }).unwrap();
        s.send(&Msg::File { size: 1, name: "../evil".into() }).unwrap();
        s.send(&Msg::Data(vec![1])).unwrap();
        s.send(&Msg::End).unwrap();
        assert_eq!(s.recv().unwrap(), Msg::Done { saved: 1 });
        h.join().unwrap().unwrap();
        assert!(dst.join("evil").exists());
        assert!(!dst.parent().unwrap().join("evil").exists());
    }

    #[test]
    fn pairing_only_when_enabled() {
        let (dl, cfg) = (tmp(), tmp());
        let l = Listener::bind_with(0, dl, cfg.clone()).unwrap();
        let (pairing, port) = (l.pairing.clone(), l.port());
        std::thread::spawn(move || l.run(|_| {}));
        let join = |code: &str| pair::join(&mut TcpStream::connect(("127.0.0.1", port)).unwrap(), code);
        assert!(join("12345678").is_err());
        *pairing.lock().unwrap() = Some(("12345678".into(), Instant::now()));
        let secret = join("12345678").unwrap();
        assert!(pairing.lock().unwrap().is_none());
        assert!(join("12345678").is_err());
        let t = Instant::now();
        let saved = loop {
            match config::load_peer_from(&cfg) {
                Ok(p) => break p,
                Err(_) if t.elapsed() < Duration::from_secs(2) => std::thread::sleep(Duration::from_millis(20)),
                Err(e) => panic!("{e}"),
            }
        };
        assert_eq!((saved.host.as_str(), saved.secret), ("127.0.0.1", secret));
    }

    #[test]
    fn busy_port_message() {
        let a = Listener::bind_with(0, tmp(), tmp()).unwrap();
        let e = Listener::bind_with(a.port(), tmp(), tmp()).err().unwrap();
        assert!(e.contains("already running"), "{e}");
    }
}
