//! Sending files (client) and receiving them (listener, pairing mode). No GUI imports.
use crate::config::{self, Partner};
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
    /// A batch from a partner (named) was accepted and is being received.
    Receiving(String),
    Received(Received),
    /// Paired with a new partner (its name).
    Paired(String),
    Refused(IpAddr),
    Error(String),
    /// A pairing attempt failed (as opposed to an unrelated connection).
    PairFailed(String),
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
    let addr = config::resolve(host, crate::PORT)?;
    let s = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|_| "other PC unreachable".to_string())?;
    timeouts(&s);
    Ok(s)
}

/// Pair with the PC showing `code`, and add it to the partners.
pub fn pair_with(host: &str, code: &str) -> Result<Partner, String> {
    let code = pair::parse_code(code)?;
    let mut s = connect(host)?;
    let (who, secret) = pair::join(&mut s, &code, &pair::Who::this_pc())?;
    let p = Partner { id: pair::partner_id(&secret), host: host.to_string(), secret, os: who.os, name: who.name };
    config::add_partner(p.clone())?;
    Ok(p)
}

/// How often the tray re-checks which partners are online.
pub const PRESENCE_EVERY: Duration = Duration::from_secs(5);
const PING_TIMEOUT: Duration = Duration::from_secs(1);

/// True only if `p` answers a presence ping (`H` + id + handshake) within about a second.
pub fn ping(p: &Partner) -> bool {
    // ponytail: the DNS lookup in resolve has no timeout; a slow resolver stalls the presence
    // thread (not the tray loop). Resolve on a thread with a deadline if that bites.
    let Ok(addr) = config::resolve(&p.host, crate::PORT) else { return false };
    let Ok(s) = TcpStream::connect_timeout(&addr, PING_TIMEOUT) else { return false };
    let _ = s.set_read_timeout(Some(PING_TIMEOUT));
    let _ = s.set_write_timeout(Some(PING_TIMEOUT));
    Session::client(s, b'H', &p.id, &p.secret).is_ok()
}

/// The partners that answer a ping, pinged in parallel, in list order.
pub fn online(ps: &[Partner]) -> Vec<Partner> {
    std::thread::scope(|sc| {
        let hs: Vec<_> = ps.iter().map(|p| sc.spawn(move || ping(p))).collect();
        ps.iter().zip(hs).filter_map(|(p, h)| h.join().unwrap_or(false).then(|| p.clone())).collect()
    })
}

/// Every `PRESENCE_EVERY`, refresh `online` from the partners file; `changed` runs when the set of ids changes.
pub fn watch_online(online: Arc<Mutex<Vec<Partner>>>, changed: impl Fn() + Send + 'static) -> ! {
    let ids = |v: &[Partner]| v.iter().map(|p| p.id).collect::<Vec<_>>();
    loop {
        let now = self::online(&config::load_partners().unwrap_or_default());
        let mut cur = online.lock().unwrap_or_else(|e| e.into_inner());
        let differs = ids(&cur) != ids(&now);
        *cur = now;
        drop(cur);
        if differs {
            changed();
        }
        std::thread::sleep(PRESENCE_EVERY);
    }
}

pub enum Target {
    Nobody,
    One(Partner),
    Several(Vec<Partner>),
}

/// Who a send goes to, from the partners online now; `to` picks one of them by name.
pub fn target(mut online: Vec<Partner>, to: Option<&str>) -> Result<Target, String> {
    Ok(match (online.len(), to) {
        (0, _) => Target::Nobody,
        (_, Some(to)) => Target::One(config::find(&online, to)?.clone()),
        (1, None) => Target::One(online.remove(0)),
        _ => Target::Several(online),
    })
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

pub fn send_files(p: &Partner, paths: &[PathBuf]) -> Result<String, String> {
    let count = u16::try_from(paths.len()).map_err(|_| "not sent: too many files at once".to_string())?;
    let stream = connect(&p.host).map_err(|e| format!("not sent: {e}"))?;
    let mut sess = Session::client(stream, b'S', &p.id, &p.secret)
        .map_err(|_| "not sent: the other PC refused (pairing doesn't match: pair again)".to_string())?;
    if let Err((network, e)) = send_batch(&mut sess, count, paths) {
        // Only a failed write can mean the receiver hung up with a reason; local errors have none pending.
        if network {
            if let Ok(Msg::Fail(r)) = sess.recv() {
                return Err(format!("not sent: {r}"));
            }
        }
        return Err(format!("not sent: {e}"));
    }
    match sess.recv() {
        Ok(Msg::Done { saved }) if saved == count => {
            Ok(format!("{count} file{} sent to {}", if count == 1 { "" } else { "s" }, p.name))
        }
        Ok(Msg::Fail(r)) => Err(format!("not sent: {r}")),
        Ok(_) => Err("not sent: the other PC did not confirm".into()),
        Err(e) => Err(format!("not sent: {e}")),
    }
}

/// Err is (came from the network write, message).
fn send_batch(sess: &mut Session<TcpStream>, count: u16, paths: &[PathBuf]) -> Result<(), (bool, String)> {
    let (net, local) = (|e| (true, e), |e| (false, e));
    sess.send(&Msg::Batch { count, from: trunc(&config::this_pc_name(), 1000) }).map_err(net)?;
    for p in paths {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let f = std::fs::File::open(p).map_err(|e| local(format!("cannot read {}: {e}", p.display())))?;
        let size = f.metadata().map_err(|e| local(format!("cannot read {}: {e}", p.display())))?.len();
        sess.send(&Msg::File { size, name: name.clone() }).map_err(net)?;
        let (mut f, mut buf, mut sent) = (f.take(size), vec![0u8; CHUNK], 0u64);
        loop {
            let n = f.read(&mut buf).map_err(|e| local(format!("cannot read {}: {e}", p.display())))?;
            if n == 0 {
                break;
            }
            sess.send(&Msg::Data(buf[..n].to_vec())).map_err(net)?;
            sent += n as u64;
        }
        if sent != size {
            return Err(local(format!("{name} changed while sending")));
        }
        sess.send(&Msg::End).map_err(net)?;
    }
    Ok(())
}

/// One batch of files from a connected, mode-byte-consumed stream into `dir`.
/// `started` is called with the sender's name once the batch header arrives.
pub fn serve_one(stream: TcpStream, secret: &[u8; 32], dir: &Path, started: &dyn Fn(&str)) -> Result<Received, String> {
    timeouts(&stream);
    let keep = stream.try_clone().map_err(|e| e.to_string())?;
    let mut sess = Session::server(stream, secret)?;
    match receive(&mut sess, dir, started) {
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

fn receive(sess: &mut Session<TcpStream>, dir: &Path, started: &dyn Fn(&str)) -> Result<Received, String> {
    let Msg::Batch { count, from } = sess.recv()? else { return Err("unexpected message".into()) };
    started(&from);
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
                "port {port} is busy: synergy-tools is probably already running, or the old synergy-dragndrop (quit it first)"
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
                let saved = pair::host(&mut s, &code, &pair::Who::this_pc()).and_then(|(who, secret)| {
                    let p = Partner { id: pair::partner_id(&secret), host: ip.to_string(), secret, os: who.os, name: who.name };
                    config::add_partner_to(&self.config_dir, p.clone()).map(|()| p.name)
                });
                on_event(match saved {
                    Ok(name) => Event::Paired(name),
                    Err(e) => Event::PairFailed(e),
                });
            }
            // Presence ping: a handshake proves the partner is here; never an event.
            b'H' => {
                let mut id = [0u8; 8];
                if s.read_exact(&mut id).is_err() {
                    return;
                }
                let known = config::load_partners_from(&self.config_dir).ok().and_then(|ps| ps.into_iter().find(|p| p.id == id));
                if let Some(p) = known {
                    let _ = Session::server(s, &p.secret);
                }
            }
            b'S' => {
                let mut id = [0u8; 8];
                if s.read_exact(&mut id).is_err() {
                    return;
                }
                let secret = match config::load_partners_from(&self.config_dir) {
                    Ok(ps) => match ps.into_iter().find(|p| p.id == id) {
                        Some(p) => p.secret,
                        None => return on_event(Event::Refused(ip)),
                    },
                    Err(e) => return on_event(Event::Error(e)),
                };
                on_event(match serve_one(s, &secret, &self.downloads, &|from| on_event(Event::Receiving(from.to_string()))) {
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
    fn serve(secret: [u8; 32], dir: PathBuf) -> (Partner, std::thread::JoinHandle<Result<Received, String>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let host = format!("127.0.0.1:{}", l.local_addr().unwrap().port());
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut mode_id = [0u8; 9];
            s.read_exact(&mut mode_id).unwrap();
            serve_one(s, &secret, &dir, &|_| {})
        });
        (partner(&host, [0; 32]), h)
    }

    fn partner(host: &str, secret: [u8; 32]) -> Partner {
        Partner { id: pair::partner_id(&secret), host: host.into(), secret, os: "linux".into(), name: "pc".into() }
    }

    fn peer_for(p: Partner, secret: [u8; 32]) -> Partner {
        Partner { secret, ..p }
    }

    fn who(name: &str) -> pair::Who {
        pair::Who { name: name.into(), os: "linux".into() }
    }

    /// Partners in `cfg` once there are `n` of them (the listener saves from its own thread).
    fn wait_partners(cfg: &Path, n: usize) -> Vec<Partner> {
        let t = Instant::now();
        loop {
            let ps = config::load_partners_from(cfg).unwrap();
            if ps.len() == n || t.elapsed() > Duration::from_secs(2) {
                return ps;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
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
        let e = send_files(&partner("127.0.0.1:1", [0; 32]), &[f]).unwrap_err();
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
        let mut s = Session::client(TcpStream::connect(&p.host).unwrap(), b'S', &[0; 8], &[3; 32]).unwrap();
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
    fn local_read_error_returns_at_once() {
        let src = tmp();
        std::fs::write(src.join("a"), b"a").unwrap();
        let (p, _h) = serve([8; 32], tmp());
        let t = Instant::now();
        let e = send_files(&peer_for(p, [8; 32]), &[src.join("a"), src.join("gone")]).unwrap_err();
        assert!(e.starts_with("not sent: cannot read") && e.contains("gone"), "{e}");
        assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    }

    #[test]
    fn hostile_name_stays_in_dir() {
        let dst = tmp();
        let (p, h) = serve([6; 32], dst.clone());
        let mut s = Session::client(TcpStream::connect(&p.host).unwrap(), b'S', &[0; 8], &[6; 32]).unwrap();
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
    fn receiving_event_precedes_received() {
        let (dl, cfg, src) = (tmp(), tmp(), tmp());
        let l = Listener::bind_with(0, dl, cfg.clone()).unwrap();
        let port = l.port();
        config::save_partners_to(&cfg, &[partner("127.0.0.1", [7; 32])]).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        std::thread::spawn(move || l.run(move |e| tx.lock().unwrap().send(e).unwrap()));
        std::fs::write(src.join("a.txt"), b"hi").unwrap();
        let p = partner(&format!("127.0.0.1:{port}"), [7; 32]);
        send_files(&p, &[src.join("a.txt")]).unwrap();
        let first = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(first, Event::Receiving(_)), "{first:?}");
        let second = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(second, Event::Received(_)), "{second:?}");
    }

    #[test]
    fn pairing_only_when_enabled() {
        let (dl, cfg) = (tmp(), tmp());
        let l = Listener::bind_with(0, dl, cfg.clone()).unwrap();
        let (pairing, port) = (l.pairing.clone(), l.port());
        std::thread::spawn(move || l.run(|_| {}));
        let join = |code: &str| pair::join(&mut TcpStream::connect(("127.0.0.1", port)).unwrap(), code, &who("pc j"));
        assert!(join("12345678").is_err());
        *pairing.lock().unwrap() = Some(("12345678".into(), Instant::now()));
        let (_, secret) = join("12345678").unwrap();
        assert!(pairing.lock().unwrap().is_none());
        assert!(join("12345678").is_err());
        let saved = wait_partners(&cfg, 1);
        assert_eq!(saved.len(), 1);
        let p = &saved[0];
        assert_eq!((p.host.as_str(), p.secret, p.id, p.name.as_str()), ("127.0.0.1", secret, pair::partner_id(&secret), "pc j"));
    }

    #[test]
    fn second_pairing_adds_partner() {
        let (dl, cfg) = (tmp(), tmp());
        let l = Listener::bind_with(0, dl, cfg.clone()).unwrap();
        let (pairing, port) = (l.pairing.clone(), l.port());
        std::thread::spawn(move || l.run(|_| {}));
        for name in ["pc-a", "pc-b"] {
            *pairing.lock().unwrap() = Some(("12345678".into(), Instant::now()));
            pair::join(&mut TcpStream::connect(("127.0.0.1", port)).unwrap(), "12345678", &who(name)).unwrap();
            wait_partners(&cfg, if name == "pc-a" { 1 } else { 2 });
        }
        let names: Vec<_> = config::load_partners_from(&cfg).unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["pc-a", "pc-b"]);
    }

    #[test]
    fn unknown_partner_id_refused() {
        let (dl, cfg, src) = (tmp(), tmp(), tmp());
        let l = Listener::bind_with(0, dl.clone(), cfg.clone()).unwrap();
        let port = l.port();
        config::save_partners_to(&cfg, &[partner("127.0.0.1", [7; 32])]).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        std::thread::spawn(move || l.run(move |e| tx.lock().unwrap().send(e).unwrap()));
        std::fs::write(src.join("a.txt"), b"hi").unwrap();
        let stranger = Partner { id: [9; 8], ..partner(&format!("127.0.0.1:{port}"), [7; 32]) };
        assert!(send_files(&stranger, &[src.join("a.txt")]).is_err());
        let e = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(e, Event::Refused(_)), "{e:?}");
        assert_eq!(count(&dl), 0);
    }

    /// A running listener that knows `ps`; returns its port and its event channel.
    fn listener_with(ps: &[Partner]) -> (u16, std::sync::mpsc::Receiver<Event>) {
        let (dl, cfg) = (tmp(), tmp());
        let l = Listener::bind_with(0, dl, cfg.clone()).unwrap();
        let port = l.port();
        config::save_partners_to(&cfg, ps).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        std::thread::spawn(move || l.run(move |e| tx.lock().unwrap().send(e).unwrap()));
        (port, rx)
    }

    #[test]
    fn ping_known_partner_true() {
        let (port, rx) = listener_with(&[partner("127.0.0.1", [7; 32])]);
        let p = partner(&format!("127.0.0.1:{port}"), [7; 32]);
        assert!(ping(&p));
        // The ping made no event: the first one comes from the send that follows.
        let f = tmp().join("a");
        std::fs::write(&f, b"a").unwrap();
        send_files(&p, &[f]).unwrap();
        let e = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(e, Event::Receiving(_)), "{e:?}");
    }

    #[test]
    fn ping_unknown_id_false() {
        let (port, rx) = listener_with(&[partner("127.0.0.1", [7; 32])]);
        let stranger = Partner { id: [9; 8], ..partner(&format!("127.0.0.1:{port}"), [7; 32]) };
        assert!(!ping(&stranger));
        // Closed silently: no Refused before the next real send's events.
        let (f, p) = (tmp().join("a"), partner(&format!("127.0.0.1:{port}"), [7; 32]));
        std::fs::write(&f, b"a").unwrap();
        send_files(&p, &[f]).unwrap();
        let e = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(e, Event::Receiving(_)), "{e:?}");
    }

    #[test]
    fn ping_nobody_listening_false_fast() {
        let t = Instant::now();
        assert!(!ping(&partner("127.0.0.1:1", [0; 32])));
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    }

    #[test]
    fn online_lists_only_answering() {
        let (port, _rx) = listener_with(&[partner("127.0.0.1", [7; 32])]);
        let up = Partner { name: "up".into(), ..partner(&format!("127.0.0.1:{port}"), [7; 32]) };
        let down = Partner { name: "down".into(), ..partner("127.0.0.1:1", [8; 32]) };
        let names: Vec<_> = online(&[down, up]).into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["up"]);
    }

    #[test]
    fn target_by_online_count() {
        let named = |n: &str, s: u8| Partner { name: n.into(), ..partner("h", [s; 32]) };
        let (a, b) = (named("a", 1), named("b", 2));
        assert!(matches!(target(vec![], None), Ok(Target::Nobody)));
        assert!(matches!(target(vec![], Some("a")), Ok(Target::Nobody)));
        assert!(matches!(target(vec![a.clone()], None), Ok(Target::One(p)) if p.name == "a"));
        assert!(matches!(target(vec![a.clone(), b.clone()], None), Ok(Target::Several(v)) if v.len() == 2));
        assert!(matches!(target(vec![a.clone(), b.clone()], Some("b")), Ok(Target::One(p)) if p.name == "b"));
        let e = target(vec![a, b], Some("zz")).err().unwrap();
        assert!(e.contains("a (linux)") && e.contains("b (linux)"), "{e}");
    }

    #[test]
    fn busy_port_message() {
        let a = Listener::bind_with(0, tmp(), tmp()).unwrap();
        let e = Listener::bind_with(a.port(), tmp(), tmp()).err().unwrap();
        assert!(e.contains("synergy-tools is probably already running"), "{e}");
        assert!(e.contains("or the old synergy-dragndrop"), "{e}");
    }
}
