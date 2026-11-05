//! synergy-audio: stream system audio from one machine to another over UDP, encrypted.
//!
//! Pairing (`pair`, once per sender/receiver): SPAKE2 with an 8-digit code, `PAIR1`..`PAIR4`,
//! gives both sides a 32-byte secret; the sender picks an 8-byte peer id.
//! Session: Noise NNpsk0 with that secret as PSK. `HELLO` + peer id + Noise message 1
//! (sender), `HELLO_REPLY` + message 2 (receiver). After that every datagram, both ways, is
//! [b'D'][nonce: u64 LE][ChaChaPoly(payload) + 16-byte tag]:
//! audio (sender -> receiver, nonce = seq, payload = 240 stereo i16 LE frames = 5 ms at
//! 48 kHz, 985 B in all), keepalive (sender -> receiver every 500 ms, nonce with `KA_BIT` set,
//! empty payload) and ack (receiver -> sender in reply, its own nonce counter, empty payload).

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig, SupportedStreamConfig};
use sha2::{Digest, Sha256};
use snow::{HandshakeState, StatelessTransportState};
use spake2::{Ed25519Group, Identity, Password, Spake2};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const PORT: u16 = 24850;
const RATE: u32 = 48_000;
const FRAMES_PER_PACKET: usize = 240; // 5 ms at 48 kHz
const SAMPLES_PER_PACKET: usize = FRAMES_PER_PACKET * 2; // stereo
const SAMPLES_PER_MS: usize = RATE as usize / 1000 * 2;
const DEFAULT_BUFFER_MS: usize = 20;
const STATS_EVERY: Duration = Duration::from_secs(5);
const SEND_QUEUE: usize = 20; // packets between capture callback and sender thread (~100 ms)
const LOG_MAX: u64 = 1 << 20; // --log file is emptied when it grows past 1 MiB
const RETRY_EVERY: Duration = Duration::from_secs(10);
const KEEPALIVE_EVERY: Duration = Duration::from_millis(500);
const ACK_FRESH: Duration = Duration::from_millis(1500); // newer ack = receiver is playing us
const REHANDSHAKE: Duration = Duration::from_secs(3); // no ack for this long: new handshake
const PAIR_TIMEOUT: Duration = Duration::from_secs(120);
const NOISE: &str = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s";
const HELLO: &[u8] = b"SAh1";
const HELLO_REPLY: &[u8] = b"SAh2";
const PAIR1: &[u8] = b"SAp1"; // sender: SPAKE2 message A
const PAIR2: &[u8] = b"SAp2"; // receiver: SPAKE2 message B + its confirmation
const PAIR3: &[u8] = b"SAp3"; // sender: peer id + its confirmation
const PAIR4: &[u8] = b"SAp4"; // receiver: confirmed and saved
const KA_BIT: u64 = 1 << 63; // keepalive nonces; audio seqs never get that high

/// Status line to the --log file if one is open, else stderr (see `say_line`).
macro_rules! say {
    ($($t:tt)*) => { say_line(format!($($t)*)) };
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let res = match args.as_slice() {
        ["--version" | "-V"] => {
            println!("synergy-audio {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        ["pair"] => pair_receive(),
        ["pair", host, code] => pair_send(host, code),
        ["unpair"] => unpair(),
        ["send", target] => send(target),
        ["send", target, "--log", path] => send_logged(target, path),
        ["unmute"] => {
            mute::restore();
            Ok(())
        }
        ["receive"] => receive(DEFAULT_BUFFER_MS),
        ["receive", "--buffer-ms", n] => match n.parse() {
            Ok(n) if n > 0 => receive(n),
            _ => Err(format!("--buffer-ms needs a positive integer, got {n:?}")),
        },
        _ => Err(format!(
            "usage:\n  synergy-audio pair   (on the receiver: shows a code, waits for the sender)\n  \
             synergy-audio pair <receiver-host[:port]> <code>   (on the sender, once)\n  \
             synergy-audio unpair   (delete all pairings on this machine)\n  \
             synergy-audio send <host[:port]> [--log <file>]   (default port {PORT})\n  \
             synergy-audio receive [--buffer-ms N]   (default {DEFAULT_BUFFER_MS})\n  \
             synergy-audio unmute   (undo the sender's auto-mute after it was killed)\n  \
             synergy-audio --version\n\
             --log: write status to <file> instead of the terminal (emptied when it passes 1 MiB),\n  \
             retry every {}s on errors, and on Windows detach from the console window",
            RETRY_EVERY.as_secs()
        )),
    };
    if let Err(e) = res {
        say!("error: {e}");
        std::process::exit(1);
    }
}

// ---------- status output ----------

static LOG: OnceLock<Mutex<File>> = OnceLock::new();

/// Status line to the --log file if one is open, else stderr.
fn say_line(line: String) {
    match LOG.get() {
        Some(f) => {
            let _ = log_line(&mut f.lock().unwrap(), &line, LOG_MAX);
        }
        None => eprintln!("{line}"),
    }
}

/// Append a line; once the file is past `max` bytes, start it over from empty.
/// (A file opened in append mode can't be truncated on Windows, so the file is opened
/// with plain write access and positioned at the end instead.)
fn log_line(f: &mut File, line: &str, max: u64) -> std::io::Result<()> {
    if f.metadata()?.len() > max {
        f.set_len(0)?;
        f.seek(SeekFrom::Start(0))?;
    }
    writeln!(f, "{line}")
}

fn open_log(path: &str) -> Result<File, String> {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .map_err(|e| format!("can't open log {path:?}: {e}"))?;
    f.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
    Ok(f)
}

/// Unattended sender (autostart): log to a file, no console window, keep retrying.
fn send_logged(target: &str, path: &str) -> Result<(), String> {
    LOG.set(Mutex::new(open_log(path)?)).unwrap();
    detach_console();
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    say!("--- synergy-audio send started (unix time {secs}) ---");
    // At login the audio device or network may not be ready yet; Task Scheduler does not
    // restart a task whose process exits with an error, so retry here instead.
    loop {
        if let Err(e) = send(target) {
            say!("error: {e} (retrying in {}s)", RETRY_EVERY.as_secs());
        }
        std::thread::sleep(RETRY_EVERY);
    }
}

/// Close the console window Windows opens for a console program started by Task Scheduler.
/// The binary stays a console program so manual runs in a terminal still print.
#[cfg(windows)]
fn detach_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn FreeConsole() -> i32;
    }
    unsafe {
        FreeConsole();
    }
}

#[cfg(not(windows))]
fn detach_console() {}

// ---------- packet ----------

fn pcm_bytes(samples: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

fn pcm_samples(buf: &[u8]) -> Option<Vec<i16>> {
    if !buf.len().is_multiple_of(4) {
        return None; // not whole stereo frames
    }
    Some(buf.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect())
}

/// Encrypt one transport packet: [b'D'][nonce LE][ciphertext + tag].
fn seal(t: &StatelessTransportState, nonce: u64, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0; 9 + payload.len() + 16];
    out[0] = b'D';
    out[1..9].copy_from_slice(&nonce.to_le_bytes());
    t.write_message(nonce, payload, &mut out[9..]).expect("packet fits");
    out
}

/// Decrypt and authenticate a transport packet; None for anything else.
fn open(t: &StatelessTransportState, buf: &[u8]) -> Option<(u64, Vec<u8>)> {
    if buf.len() < 9 + 16 || buf[0] != b'D' {
        return None;
    }
    let nonce = u64::from_le_bytes(buf[1..9].try_into().unwrap());
    let mut payload = vec![0; buf.len() - 9 - 16];
    t.read_message(nonce, &buf[9..], &mut payload).ok()?;
    Some((nonce, payload))
}

fn noise(psk: &[u8; 32]) -> snow::Builder<'_> {
    snow::Builder::new(NOISE.parse().unwrap()).psk(0, psk).unwrap()
}

/// Sender: start a handshake. Returns the state to finish it with and the hello datagram.
fn hello(peer: &[u8; 8], psk: &[u8; 32]) -> (HandshakeState, Vec<u8>) {
    let mut hs = noise(psk).build_initiator().unwrap();
    let mut msg = [0u8; 128];
    let n = hs.write_message(&[], &mut msg).unwrap();
    (hs, [HELLO, peer, &msg[..n]].concat())
}

/// Sender: the receiver's reply (after `HELLO_REPLY`) completes the handshake.
fn finish_hello(mut hs: HandshakeState, reply: &[u8]) -> Option<StatelessTransportState> {
    hs.read_message(reply, &mut [0u8; 128]).ok()?;
    hs.into_stateless_transport_mode().ok()
}

/// One sender's session on the receiver.
struct Sess {
    t: StatelessTransportState,
    peer: [u8; 8],
    last_audio: Option<u64>,
    last_ka: Option<u64>,
    acks: u64, // nonce of the next ack
}

/// What the receiver does with one datagram.
#[derive(Debug, PartialEq)]
enum In {
    Send(Vec<u8>), // hello reply or ack, back to where the datagram came from
    Audio(Vec<i16>),
    Late,     // authentic but late, duplicate or replayed
    Rejected, // unknown peer, bad MAC, malformed
}

/// Receiver session state. A new handshake goes into `next` and replaces `cur` only once a
/// packet authenticates under it, so a replayed old hello can't break a running stream.
struct Rx {
    peers: HashMap<[u8; 8], [u8; 32]>,
    cur: Option<Sess>,
    next: Option<Sess>,
    promoted: u64, // sessions started, for the log line
}

impl Rx {
    fn handle(&mut self, buf: &[u8]) -> In {
        if let Some(rest) = buf.strip_prefix(HELLO) {
            let Some((peer, msg)) = rest.split_first_chunk::<8>() else { return In::Rejected };
            let Some(psk) = self.peers.get(peer) else { return In::Rejected };
            let mut hs = noise(psk).build_responder().unwrap();
            let mut reply = [0u8; 128];
            let Some(n) = hs.read_message(msg, &mut [0u8; 128]).ok().and_then(|_| hs.write_message(&[], &mut reply).ok())
            else {
                return In::Rejected;
            };
            let Ok(t) = hs.into_stateless_transport_mode() else { return In::Rejected };
            self.next = Some(Sess { t, peer: *peer, last_audio: None, last_ka: None, acks: 0 });
            return In::Send([HELLO_REPLY, &reply[..n]].concat());
        }
        let (nonce, payload) = if let Some(p) = self.cur.as_ref().and_then(|s| open(&s.t, buf)) {
            p
        } else if let Some(p) = self.next.as_ref().and_then(|s| open(&s.t, buf)) {
            self.cur = self.next.take();
            self.promoted += 1;
            p
        } else {
            return In::Rejected;
        };
        let s = self.cur.as_mut().unwrap();
        if nonce & KA_BIT != 0 {
            if !accept(s.last_ka, nonce) {
                return In::Late;
            }
            s.last_ka = Some(nonce);
            s.acks += 1;
            return In::Send(seal(&s.t, s.acks - 1, &[]));
        }
        if !accept(s.last_audio, nonce) {
            return In::Late;
        }
        s.last_audio = Some(nonce);
        pcm_samples(&payload).map_or(In::Rejected, In::Audio)
    }
}

/// Accept a packet unless it is late, a duplicate or a replay. A restarted sender makes a
/// new session (new keys, counters start over), so within one session counters only grow.
fn accept(last: Option<u64>, seq: u64) -> bool {
    last.is_none_or(|last| seq > last)
}

// ---------- pairing ----------

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0; N];
    getrandom::fill(&mut b).expect("no OS random number source");
    b
}

fn digest(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

fn spake(code: &str, sender: bool) -> (Spake2<Ed25519Group>, Vec<u8>) {
    let (pw, a, b) = (Password::new(code), Identity::new(b"synergy-audio sender"), Identity::new(b"synergy-audio receiver"));
    if sender { Spake2::start_a(&pw, &a, &b) } else { Spake2::start_b(&pw, &a, &b) }
}

/// Receiver side of pairing: one attempt, from the first address that starts one.
/// Returns the sender's peer id and the pairing secret.
fn pair_receiver(sock: &UdpSocket, code: &str, timeout: Duration) -> Result<([u8; 8], [u8; 32]), String> {
    sock.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    let deadline = Instant::now() + timeout;
    let mut buf = [0u8; 2048];
    let mut state: Option<(SocketAddr, Vec<u8>, Vec<u8>)> = None; // sender, our reply, key
    while Instant::now() < deadline {
        let Ok((n, from)) = sock.recv_from(&mut buf) else { continue };
        let m = &buf[..n];
        match &state {
            None => {
                let Some(msg_a) = m.strip_prefix(PAIR1) else { continue };
                let (s, msg_b) = spake(code, false);
                let key = s.finish(msg_a).map_err(|_| "pairing failed: bad message from the sender")?;
                let reply = [PAIR2, &msg_b, &digest(&[b"synergy-audio confirm B", &key])].concat();
                let _ = sock.send_to(&reply, from);
                state = Some((from, reply, key));
            }
            Some((addr, reply, _)) if from == *addr && m.starts_with(PAIR1) => {
                let _ = sock.send_to(reply, from); // our reply got lost: same answer, no new attempt
            }
            Some((addr, _, key)) if from == *addr => {
                let Some((peer, conf)) = m.strip_prefix(PAIR3).and_then(|r| r.split_first_chunk::<8>()) else {
                    continue;
                };
                if conf != digest(&[b"synergy-audio confirm A", key, peer]) {
                    return Err("pairing failed: wrong code (start again with a new code)".into());
                }
                let _ = sock.send_to(PAIR4, from);
                return Ok((*peer, digest(&[b"synergy-audio secret", key])));
            }
            Some(_) => {}
        }
    }
    Err("pairing timed out: no sender tried within 2 minutes".into())
}

/// Send `msg` (resent every second, 10 tries) until a reply starting with `want` arrives.
fn exchange(sock: &UdpSocket, msg: &[u8], want: &[u8]) -> Result<Vec<u8>, String> {
    let mut buf = [0u8; 2048];
    for _ in 0..10 {
        let _ = sock.send(msg);
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            sock.set_read_timeout(Some(left)).unwrap();
            match sock.recv(&mut buf) {
                Ok(n) if buf[..n].starts_with(want) => return Ok(buf[want.len()..n].to_vec()),
                Ok(_) => {}
                Err(_) => std::thread::sleep(left), // e.g. ICMP port unreachable: wait out the second
            }
        }
    }
    Err(format!("no answer from the receiver: is `synergy-audio pair` running there, and UDP {PORT} open?"))
}

/// Sender side of pairing over a connected socket. Returns our new peer id and the secret.
fn pair_sender(sock: &UdpSocket, code: &str) -> Result<([u8; 8], [u8; 32]), String> {
    let (s, msg_a) = spake(code, true);
    let reply = exchange(sock, &[PAIR1, &msg_a].concat(), PAIR2)?;
    let (msg_b, conf) = reply.split_at_checked(reply.len().saturating_sub(32)).ok_or("bad reply")?;
    let key = s.finish(msg_b).map_err(|_| "pairing failed: bad reply from the receiver")?;
    let peer: [u8; 8] = random();
    let msg3 = [PAIR3, &peer, &digest(&[b"synergy-audio confirm A", &key, &peer])].concat();
    if conf != digest(&[b"synergy-audio confirm B", &key]) {
        let _ = sock.send(&msg3); // the receiver sees the mismatch too and stops
        return Err("pairing failed: wrong code (start again on the receiver for a new code)".into());
    }
    // ponytail: if only PAIR4 is lost the receiver is paired but we report failure; pair again.
    exchange(sock, &msg3, PAIR4)?;
    Ok((peer, digest(&[b"synergy-audio secret", &key])))
}

fn config_dir() -> Result<PathBuf, String> {
    let var = |k| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        var("APPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library/Application Support"))
    } else {
        var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|h| h.join(".config")))
    };
    base.map(|b| b.join("synergy-audio")).ok_or("can't find the config directory (HOME / APPDATA not set)".into())
}

/// Write a pairing file, readable only by this user on unix (0600).
fn write_secret(path: &Path, text: &str) -> Result<(), String> {
    let err = |e: std::io::Error| format!("can't write {}: {e}", path.display());
    std::fs::create_dir_all(path.parent().unwrap()).map_err(err)?;
    let _ = std::fs::remove_file(path); // so the mode below applies
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(path).and_then(|mut f| f.write_all(text.as_bytes())).map_err(err)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex<const N: usize>(s: &str) -> Option<[u8; N]> {
    let mut out = [0; N];
    if s.len() != 2 * N {
        return None;
    }
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

const NOT_PAIRED: &str = "run `synergy-audio pair` on the receiver, then `synergy-audio pair <receiver-host> <code>` on the sender";

/// Sender: the one paired receiver file: "<host> <peer id> <secret>".
fn load_receiver() -> Result<(String, [u8; 8], [u8; 32]), String> {
    let path = config_dir()?.join("receiver");
    let text = std::fs::read_to_string(&path).map_err(|_| format!("not paired yet ({} missing): {NOT_PAIRED}", path.display()))?;
    match text.split_whitespace().collect::<Vec<_>>()[..] {
        [host, peer, secret] => match (unhex(peer), unhex(secret)) {
            (Some(p), Some(s)) => Ok((host.to_string(), p, s)),
            _ => Err(format!("{} is damaged: {NOT_PAIRED}", path.display())),
        },
        _ => Err(format!("{} is damaged: {NOT_PAIRED}", path.display())),
    }
}

/// Receiver: one file per paired sender, named by its peer id, holding the secret.
fn load_senders() -> Result<HashMap<[u8; 8], [u8; 32]>, String> {
    let dir = config_dir()?.join("senders");
    let peers: HashMap<_, _> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let peer = unhex(e.file_name().to_str()?)?;
            Some((peer, unhex(std::fs::read_to_string(e.path()).ok()?.trim())?))
        })
        .collect();
    if peers.is_empty() {
        return Err(format!("no sender paired yet (nothing in {}): {NOT_PAIRED}", dir.display()));
    }
    Ok(peers)
}

fn pair_receive() -> Result<(), String> {
    let sock = UdpSocket::bind(("0.0.0.0", PORT)).map_err(|e| {
        format!(
            "can't bind UDP {PORT} ({e}). Is the receiver running? Stop it first \
             (systemctl --user stop synergy-audio, or Ctrl+C), pair, then start it again."
        )
    })?;
    let code = format!("{:08}", u64::from_le_bytes(random()) % 100_000_000);
    let code_shown = format!("{}-{}", &code[..4], &code[4..]);
    println!("Pairing code: {code_shown}");
    println!("On the sender run:  synergy-audio pair <this machine's address> {code_shown}");
    println!("Waiting up to {} minutes for one attempt...", PAIR_TIMEOUT.as_secs() / 60);
    let (peer, secret) = pair_receiver(&sock, &code, PAIR_TIMEOUT)?;
    let path = config_dir()?.join("senders").join(hex(&peer));
    write_secret(&path, &format!("{}\n", hex(&secret)))?;
    println!("Paired with sender {} (saved {}).", hex(&peer), path.display());
    println!("Start the receiver (again): systemctl --user start synergy-audio, or synergy-audio receive");
    Ok(())
}

fn pair_send(target: &str, code: &str) -> Result<(), String> {
    let digits: String = code.chars().filter(|c| *c != '-' && *c != ' ').collect();
    if digits.len() != 8 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("the code is 8 digits as shown by `synergy-audio pair`, like 1234-5678; got {code:?}"));
    }
    let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    sock.connect(resolve(target)?).map_err(|e| e.to_string())?;
    let (peer, secret) = pair_sender(&sock, &digits)?;
    let path = config_dir()?.join("receiver");
    write_secret(&path, &format!("{target} {} {}\n", hex(&peer), hex(&secret)))?;
    println!("Paired with {target} (saved {}). Now run: synergy-audio send {target}", path.display());
    Ok(())
}

fn unpair() -> Result<(), String> {
    let dir = config_dir()?;
    let mut n = usize::from(std::fs::remove_file(dir.join("receiver")).is_ok());
    for e in std::fs::read_dir(dir.join("senders")).into_iter().flatten().flatten() {
        n += usize::from(std::fs::remove_file(e.path()).is_ok());
    }
    println!("Removed {n} pairing file(s) from {}.", dir.display());
    Ok(())
}

// ---------- shared device setup ----------

/// Default output device config, forced to 48 kHz or a clear error.
// ponytail: no resampling; device must run at 48 kHz. Add a resampler if users can't change it.
fn config_48k(device: &cpal::Device) -> Result<SupportedStreamConfig, String> {
    let def = device.default_output_config().map_err(|e| format!("no output config: {e}"))?;
    if def.sample_rate() == RATE {
        return Ok(def);
    }
    device
        .supported_output_configs()
        .map_err(|e| e.to_string())?
        .filter(|c| c.sample_format() == def.sample_format())
        .find_map(|c| c.try_with_sample_rate(RATE))
        .ok_or_else(|| {
            format!(
                "device '{device}' runs at {} Hz and can't do {RATE} Hz. \
                 Set it to 48000 Hz (Windows: Sound settings > device Properties > Advanced; \
                 macOS: Audio MIDI Setup; Linux/PipeWire: default.clock.rate = 48000) and retry.",
                def.sample_rate()
            )
        })
}

fn default_output() -> Result<cpal::Device, String> {
    cpal::default_host().default_output_device().ok_or("no default output device".into())
}

// ---------- send ----------

fn resolve(target: &str) -> Result<SocketAddr, String> {
    if target.contains(':') { target.to_socket_addrs() } else { (target, PORT).to_socket_addrs() }
        .map_err(|e| format!("bad target {target:?}: {e}"))?
        .find(SocketAddr::is_ipv4)
        .ok_or(format!("{target:?} has no IPv4 address"))
}

fn send(target: &str) -> Result<(), String> {
    let (paired_with, peer, psk) = load_receiver()?;
    let addr = resolve(target)?;
    let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    sock.connect(addr).map_err(|e| e.to_string())?;
    // Same socket for keepalives, so the receiver's acks come back to it.
    let ka_sock = sock.try_clone().map_err(|e| e.to_string())?;

    // On Windows (WASAPI) and macOS 14.6+ (CoreAudio) an input stream on an output device is
    // loopback capture.
    let device = default_output()?;
    let sup = config_48k(&device)?;
    let config: StreamConfig = sup.config();
    say!(
        "send: capturing '{device}' ({} ch, {:?}, {} Hz) -> {addr} (paired with {paired_with})",
        config.channels, sup.sample_format(), config.sample_rate
    );
    if config.channels > 2 {
        say!("warning: {} channels, sending only the first two (front L/R)", config.channels);
    }

    // Set by the keepalive thread once the handshake is done; audio before that is dropped.
    let session: Arc<Mutex<Option<StatelessTransportState>>> = Arc::new(Mutex::new(None));
    // The capture callback must never block, so it hands (seq, pcm) to a sender thread,
    // which encrypts and sends.
    let (tx, rx) = sync_channel::<(u64, Vec<u8>)>(SEND_QUEUE);
    let sent = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));
    let dropped = Arc::new(AtomicU64::new(0));
    {
        let (sent, errors, session) = (sent.clone(), errors.clone(), session.clone());
        std::thread::spawn(move || {
            for (seq, pcm) in rx {
                let Some(pkt) = session.lock().unwrap().as_ref().map(|t| seal(t, seq, &pcm)) else { continue };
                // Errors are expected while the receiver isn't up yet; count, don't stop.
                match sock.send(&pkt) {
                    Ok(_) => sent.fetch_add(1, Relaxed),
                    Err(_) => errors.fetch_add(1, Relaxed),
                };
            }
        });
    }
    let stream = match sup.sample_format() {
        SampleFormat::F32 => build_capture::<f32>(&device, &config, tx, dropped.clone()),
        SampleFormat::I16 => build_capture::<i16>(&device, &config, tx, dropped.clone()),
        SampleFormat::I32 => build_capture::<i32>(&device, &config, tx, dropped.clone()),
        f => return Err(format!("unsupported sample format {f:?}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;

    // Spawned last: nothing below returns, so send_logged's retries never stack threads.
    let link = Arc::new(Mutex::new(Link { last_ack: None, mute: String::new(), rejected: 0 }));
    {
        let link = link.clone();
        std::thread::spawn(move || keepalive(ka_sock, link, session, peer, psk));
    }

    let mut prev = 0;
    loop {
        std::thread::sleep(STATS_EVERY);
        let n = sent.load(Relaxed);
        let (age, mute, rejected) = {
            let l = link.lock().unwrap();
            (l.last_ack.map(|t| t.elapsed()), l.mute.clone(), l.rejected)
        };
        let receiver = match age {
            Some(a) if a < ACK_FRESH => format!("receiver ok (ack {:.1} s ago)", a.as_secs_f64()),
            Some(a) => format!("receiver not heard (last ack {:.0} s ago)", a.as_secs_f64()),
            None => "receiver not heard".into(),
        };
        say!(
            "send: {:.0} packets/s, queue-full dropped {}, send errors {}, rejected {rejected}, {receiver}, {mute}",
            (n - prev) as f64 / STATS_EVERY.as_secs_f64(),
            dropped.load(Relaxed),
            errors.load(Relaxed)
        );
        prev = n;
    }
}

/// What the keepalive thread knows, for the status line.
struct Link {
    last_ack: Option<Instant>, // last authentic ack
    mute: String,
    rejected: u64, // datagrams that failed to authenticate, or replays
}

/// Every 500 ms: (re)start the handshake if needed, send a keepalive, collect acks, then
/// mute/unmute the speakers. Runs whether or not audio is playing (loopback capture sends
/// nothing during silence).
fn keepalive(
    sock: UdpSocket,
    link: Arc<Mutex<Link>>,
    session: Arc<Mutex<Option<StatelessTransportState>>>,
    peer: [u8; 8],
    psk: [u8; 32],
) {
    mute::restore(); // a previous run that was killed may have left the speakers muted
    mute::on_ctrl_c_restore();
    let mut muted_by_us = false;
    let mut buf = [0u8; 2048];
    let (mut pending, mut heard, mut last_ack_n, mut ka_n) = (None, None::<Instant>, None, 0u64);
    loop {
        // No session yet, or no ack for a while (receiver restarted?): new handshake.
        // Errors (receiver down) just mean no answer.
        if heard.is_none_or(|t| t.elapsed() > REHANDSHAKE) {
            let (hs, msg) = hello(&peer, &psk);
            pending = Some(hs);
            let _ = sock.send(&msg);
        }
        if let Some(t) = &*session.lock().unwrap() {
            let _ = sock.send(&seal(t, KA_BIT | ka_n, &[]));
            ka_n += 1;
        }
        let deadline = Instant::now() + KEEPALIVE_EVERY;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            sock.set_read_timeout(Some(left)).unwrap();
            match sock.recv(&mut buf) {
                Ok(n) => {
                    let p = &buf[..n];
                    if let Some(reply) = p.strip_prefix(HELLO_REPLY) {
                        match pending.take().and_then(|hs| finish_hello(hs, reply)) {
                            Some(t) => {
                                *session.lock().unwrap() = Some(t);
                                (heard, last_ack_n) = (Some(Instant::now()), None);
                                say!("send: encrypted session with the receiver started");
                            }
                            None => link.lock().unwrap().rejected += 1,
                        }
                        continue;
                    }
                    let ack = session.lock().unwrap().as_ref().and_then(|t| open(t, p));
                    let mut l = link.lock().unwrap();
                    match ack {
                        Some((n, _)) if accept(last_ack_n, n) => {
                            last_ack_n = Some(n);
                            heard = Some(Instant::now());
                            l.last_ack = heard;
                        }
                        _ => l.rejected += 1,
                    }
                }
                // Timeout, or e.g. "connection reset" from an ICMP port-unreachable: wait out the tick.
                Err(_) => std::thread::sleep(deadline.saturating_duration_since(Instant::now())),
            }
        }
        let age = link.lock().unwrap().last_ack.map(|t| t.elapsed());
        let state = mute::update(age, &mut muted_by_us);
        link.lock().unwrap().mute = state;
    }
}

#[derive(Debug, PartialEq)]
enum MuteAction {
    Mute,
    Unmute,
    Nothing,
}

/// Mute while the receiver acks, unmute when it stops, and never touch a mute we didn't set.
fn mute_action(ack_age: Option<Duration>, muted: bool, muted_by_us: bool) -> MuteAction {
    let receiver_ok = ack_age.is_some_and(|a| a < ACK_FRESH);
    match (receiver_ok, muted, muted_by_us) {
        (true, false, _) => MuteAction::Mute,
        (false, true, true) => MuteAction::Unmute,
        _ => MuteAction::Nothing, // includes muted by the user: left alone, both ways
    }
}

fn build_capture<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    tx: SyncSender<(u64, Vec<u8>)>,
    dropped: Arc<AtomicU64>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    i16: FromSample<T>,
{
    let ch = config.channels as usize;
    let mut pending: Vec<i16> = Vec::with_capacity(SAMPLES_PER_PACKET * 4);
    let mut seq = 0u64;
    device
        .build_input_stream(
            *config,
            move |data: &[T], _: &_| {
                for frame in data.chunks_exact(ch) {
                    let l: i16 = frame[0].to_sample();
                    let r = if ch > 1 { frame[1].to_sample() } else { l };
                    pending.extend_from_slice(&[l, r]);
                }
                // ponytail: one small Vec alloc per packet in the callback; pool buffers if that ever glitches.
                for chunk in pending.chunks_exact(SAMPLES_PER_PACKET) {
                    // Never block here: if the sender thread is behind, drop the packet.
                    if let Err(TrySendError::Full(_)) = tx.try_send((seq, pcm_bytes(chunk))) {
                        dropped.fetch_add(1, Relaxed);
                    }
                    seq += 1;
                }
                let rem = pending.len() % SAMPLES_PER_PACKET;
                pending.drain(..pending.len() - rem);
            },
            |e| say!("capture stream error: {e}"),
            None,
        )
        .map_err(|e| format!("can't open capture stream: {e}"))
}

// ---------- speaker mute (Windows) ----------

#[cfg(windows)]
mod mute {
    use super::{mute_action, MuteAction};
    use std::path::PathBuf;
    use std::time::Duration;
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

    /// Exists exactly while we have the speakers muted, so a killed sender can be undone.
    fn marker() -> PathBuf {
        let base = std::env::var_os("LOCALAPPDATA").map_or_else(std::env::temp_dir, PathBuf::from);
        base.join("synergy-audio").join("muted-by-us")
    }

    // ponytail: looks up the default speakers on every call (2x a second); cache it if that ever shows up in a profile.
    fn endpoint() -> windows::core::Result<IAudioEndpointVolume> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED); // per thread; repeat calls are harmless
            let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            e.GetDefaultAudioEndpoint(eRender, eConsole)?.Activate(CLSCTX_ALL, None)
        }
    }

    fn set(m: bool) -> windows::core::Result<()> {
        unsafe { endpoint()?.SetMute(m, std::ptr::null()) }
    }

    /// If a marker says we left the speakers muted, unmute them and remove the marker.
    pub fn restore() {
        let m = marker();
        if m.exists() {
            match set(false) {
                Ok(()) => {
                    let _ = std::fs::remove_file(&m);
                    super::say_line("unmuted the speakers (left muted by an earlier run)".into());
                }
                Err(e) => super::say_line(format!("can't unmute the speakers: {e}")),
            }
        }
    }

    /// Ctrl+C / closing the console window: unmute before the default handler exits.
    /// (Task Scheduler stops and Stop-Process kill outright; the marker covers those.)
    pub fn on_ctrl_c_restore() {
        unsafe extern "system" fn handler(_: u32) -> i32 {
            restore();
            0 // not handled: let Windows exit the process as usual
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn SetConsoleCtrlHandler(h: Option<unsafe extern "system" fn(u32) -> i32>, add: i32) -> i32;
        }
        unsafe {
            SetConsoleCtrlHandler(Some(handler), 1);
        }
    }

    /// Apply `mute_action` to the default speakers; returns the state for the status line.
    pub fn update(ack_age: Option<Duration>, by_us: &mut bool) -> String {
        let r = (|| -> windows::core::Result<&str> {
            let muted = unsafe { endpoint()?.GetMute()?.as_bool() };
            if !muted && *by_us {
                *by_us = false; // the user unmuted it; it's no longer ours to undo
                let _ = std::fs::remove_file(marker());
            }
            Ok(match mute_action(ack_age, muted, *by_us) {
                MuteAction::Mute => {
                    let m = marker(); // marker first: a crash right after still gets undone
                    let _ = std::fs::create_dir_all(m.parent().unwrap());
                    let _ = std::fs::write(&m, "");
                    set(true)?;
                    *by_us = true;
                    "muted by us"
                }
                MuteAction::Unmute => {
                    set(false)?;
                    *by_us = false;
                    let _ = std::fs::remove_file(marker());
                    "not muted"
                }
                MuteAction::Nothing if *by_us => "muted by us",
                MuteAction::Nothing if muted => "muted by you (left alone)",
                MuteAction::Nothing => "not muted",
            })
        })();
        r.map_or_else(|e| format!("mute control failed: {e}"), str::to_string)
    }
}

/// Elsewhere (macOS sender, or Linux for testing) nothing is muted; report what Windows would do.
#[cfg(not(windows))]
mod mute {
    use super::{mute_action, MuteAction};
    use std::time::Duration;

    pub fn restore() {}
    pub fn on_ctrl_c_restore() {}
    pub fn update(ack_age: Option<Duration>, would_be_muted: &mut bool) -> String {
        match mute_action(ack_age, *would_be_muted, *would_be_muted) {
            MuteAction::Mute => *would_be_muted = true,
            MuteAction::Unmute => *would_be_muted = false,
            MuteAction::Nothing => {}
        }
        format!("no muting on this OS (Windows would {}be muted)", if *would_be_muted { "" } else { "not " })
    }
}

// ---------- receive ----------

struct Jitter {
    buf: VecDeque<i16>,
    priming: bool, // after start/underrun: stay silent until `target` is buffered
    underruns: u64,
}

fn receive(buffer_ms: usize) -> Result<(), String> {
    let target = buffer_ms * SAMPLES_PER_MS;
    // Above this the sender clock is running ahead (or a burst arrived): drop back to target.
    let max = target + target.max(10 * SAMPLES_PER_MS);

    let mut rx = Rx { peers: load_senders()?, cur: None, next: None, promoted: 0 };
    let sock = UdpSocket::bind(("0.0.0.0", PORT)).map_err(|e| format!("can't bind UDP {PORT}: {e}"))?;
    sock.set_read_timeout(Some(Duration::from_secs(1))).unwrap();

    let device = default_output()?;
    let sup = config_48k(&device)?;
    let mut config: StreamConfig = sup.config();
    // Ask for ~5 ms device periods when the backend reports a range.
    if let cpal::SupportedBufferSize::Range { min, max } = *sup.buffer_size() {
        config.buffer_size = cpal::BufferSize::Fixed((FRAMES_PER_PACKET as u32).clamp(min, max));
    }
    eprintln!(
        "receive: UDP 0.0.0.0:{PORT} -> '{device}' ({} ch, {:?}, {} Hz, device buffer {:?}), jitter buffer {buffer_ms} ms, {} paired sender(s)",
        config.channels, sup.sample_format(), config.sample_rate, config.buffer_size, rx.peers.len()
    );

    // ponytail: Mutex<VecDeque> shared with the audio callback; swap for a lock-free SPSC ring if glitches appear.
    let jb = Arc::new(Mutex::new(Jitter { buf: VecDeque::with_capacity(max * 2), priming: true, underruns: 0 }));
    let stream = match sup.sample_format() {
        SampleFormat::F32 => build_playback::<f32>(&device, &config, jb.clone(), target),
        SampleFormat::I16 => build_playback::<i16>(&device, &config, jb.clone(), target),
        SampleFormat::I32 => build_playback::<i32>(&device, &config, jb.clone(), target),
        f => return Err(format!("unsupported sample format {f:?}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;

    let mut buf = [0u8; 2048];
    let (mut pkts, mut late, mut rejected, mut trimmed) = (0u64, 0u64, 0u64, 0u64);
    let mut next_stats = Instant::now() + STATS_EVERY;
    loop {
        if let Ok((n, from)) = sock.recv_from(&mut buf) {
            let sessions = rx.promoted;
            match rx.handle(&buf[..n]) {
                In::Send(reply) => {
                    let _ = sock.send_to(&reply, from); // hello reply, or ack (the sender mutes its speakers)
                }
                In::Late => late += 1,
                In::Rejected => rejected += 1,
                In::Audio(samples) => {
                    pkts += 1;
                    let mut j = jb.lock().unwrap();
                    j.buf.extend(samples);
                    if j.buf.len() > max {
                        let excess = j.buf.len() - target;
                        j.buf.drain(..excess);
                        trimmed += 1;
                    }
                }
            }
            if rx.promoted != sessions {
                eprintln!("receive: encrypted session with sender {} at {from}", hex(&rx.cur.as_ref().unwrap().peer));
            }
        }
        if Instant::now() >= next_stats {
            let (level, underruns) = {
                let j = jb.lock().unwrap();
                (j.buf.len() / SAMPLES_PER_MS, j.underruns)
            };
            eprintln!(
                "receive: {:.0} packets/s, buffered {level} ms, underruns {underruns}, late/dup dropped {late}, rejected {rejected}, trims {trimmed}",
                pkts as f64 / STATS_EVERY.as_secs_f64()
            );
            pkts = 0;
            next_stats += STATS_EVERY;
        }
    }
}

fn build_playback<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    jb: Arc<Mutex<Jitter>>,
    target: usize,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<i16>,
{
    let ch = config.channels as usize;
    device
        .build_output_stream(
            *config,
            move |out: &mut [T], _: &_| {
                let silence = T::from_sample(0i16);
                let mut j = jb.lock().unwrap();
                if j.priming && j.buf.len() < target {
                    out.fill(silence);
                    return;
                }
                j.priming = false;
                for frame in out.chunks_exact_mut(ch) {
                    frame.fill(silence); // extra channels (>2) stay silent
                    if j.priming {
                        continue;
                    }
                    let (Some(l), Some(r)) = (j.buf.pop_front(), j.buf.pop_front()) else {
                        // Underrun: silence for the rest of this callback, then re-prime. Never block.
                        j.underruns += 1;
                        j.priming = true;
                        continue;
                    };
                    if ch == 1 {
                        frame[0] = T::from_sample(((l as i32 + r as i32) / 2) as i16);
                    } else {
                        frame[0] = T::from_sample(l);
                        frame[1] = T::from_sample(r);
                    }
                }
            },
            |e| eprintln!("playback stream error: {e}"),
            None,
        )
        .map_err(|e| format!("can't open playback stream: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_round_trip() {
        let samples: Vec<i16> = (0..SAMPLES_PER_PACKET as i32).map(|i| (i * 137 - 30000) as i16).collect();
        let pcm = pcm_bytes(&samples);
        assert_eq!(pcm.len(), 960);
        assert_eq!(pcm_samples(&pcm), Some(samples));
        assert_eq!(pcm_samples(&pcm[..9]), None);
    }

    fn pair_over_loopback(receiver_code: &str, sender_code: &str) -> (Result<([u8; 8], [u8; 32]), String>, Result<([u8; 8], [u8; 32]), String>) {
        let rsock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = rsock.local_addr().unwrap();
        let code = receiver_code.to_string();
        let r = std::thread::spawn(move || pair_receiver(&rsock, &code, Duration::from_secs(10)));
        let ssock = UdpSocket::bind("127.0.0.1:0").unwrap();
        ssock.connect(addr).unwrap();
        let s = pair_sender(&ssock, sender_code);
        (r.join().unwrap(), s)
    }

    #[test]
    fn pairing_right_code() {
        let (r, s) = pair_over_loopback("12345678", "12345678");
        assert_eq!(r.unwrap(), s.unwrap());
    }

    #[test]
    fn pairing_wrong_code_fails_on_both_sides() {
        let (r, s) = pair_over_loopback("12345678", "12345679");
        assert!(r.unwrap_err().contains("wrong code"));
        assert!(s.unwrap_err().contains("wrong code"));
    }

    #[test]
    fn session_round_trip_tamper_replay_unpaired() {
        let (peer, psk) = ([1u8; 8], [7u8; 32]);
        let mut rx = Rx { peers: HashMap::from([(peer, psk)]), cur: None, next: None, promoted: 0 };
        let (hs, hello_msg) = hello(&peer, &psk);
        let In::Send(reply) = rx.handle(&hello_msg) else { panic!("no hello reply") };
        let tx = finish_hello(hs, reply.strip_prefix(HELLO_REPLY).unwrap()).unwrap();
        let samples = vec![1i16, -2, 3, -4];
        let pkt = seal(&tx, 0, &pcm_bytes(&samples));
        assert_eq!(rx.handle(&pkt), In::Audio(samples.clone()));
        assert_eq!(rx.promoted, 1);
        assert_eq!(rx.handle(&pkt), In::Late); // replay
        let mut bad = seal(&tx, 1, &pcm_bytes(&samples));
        bad[12] ^= 1;
        assert_eq!(rx.handle(&bad), In::Rejected); // tampered
        // Keepalive -> ack the sender can authenticate; a replayed keepalive gets no ack.
        let ka = seal(&tx, KA_BIT, &[]);
        let In::Send(ack) = rx.handle(&ka) else { panic!("no ack") };
        assert_eq!(open(&tx, &ack), Some((0, vec![])));
        assert_eq!(rx.handle(&ka), In::Late);
        // Unpaired peer id, and a paired id with the wrong secret.
        assert_eq!(rx.handle(&hello(&[9; 8], &psk).1), In::Rejected);
        assert_eq!(rx.handle(&hello(&peer, &[8; 32]).1), In::Rejected);
        assert_eq!(rx.handle(b"SAka"), In::Rejected); // old plaintext keepalive
        // A replayed old hello is answered but doesn't break the running session.
        assert!(matches!(rx.handle(&hello_msg), In::Send(_)));
        assert_eq!(rx.handle(&seal(&tx, 2, &pcm_bytes(&samples))), In::Audio(samples));
        assert_eq!(rx.promoted, 1);
    }

    #[test]
    fn mute_decision() {
        use MuteAction::*;
        let fresh = Some(Duration::from_millis(300));
        let stale = Some(Duration::from_millis(1600));
        assert_eq!(mute_action(fresh, false, false), Mute);
        assert_eq!(mute_action(fresh, true, true), Nothing); // already ours
        assert_eq!(mute_action(stale, true, true), Unmute);
        assert_eq!(mute_action(None, true, true), Unmute);
        assert_eq!(mute_action(stale, false, false), Nothing);
        assert_eq!(mute_action(None, false, false), Nothing);
        // The user's own mute: never touched, whether the receiver is up or not.
        assert_eq!(mute_action(fresh, true, false), Nothing);
        assert_eq!(mute_action(stale, true, false), Nothing);
        assert_eq!(mute_action(None, true, false), Nothing);
    }

    #[test]
    fn late_and_duplicate_dropped() {
        assert!(accept(None, 0));
        assert!(accept(Some(5), 6));
        assert!(!accept(Some(5), 5)); // duplicate
        assert!(!accept(Some(5), 3)); // late
        assert!(!accept(Some(50_000), 0)); // replay of an old packet (a restart is a new session)
    }

    #[test]
    fn log_appends_then_starts_over_past_max() {
        let path = std::env::temp_dir().join(format!("synergy-audio-test-{}.log", std::process::id()));
        std::fs::write(&path, "old\n").unwrap();
        let mut f = open_log(path.to_str().unwrap()).unwrap();
        log_line(&mut f, "a", 10).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old\na\n");
        log_line(&mut f, "bbbbbbbbbb", 10).unwrap(); // 6 bytes: still under max
        log_line(&mut f, "c", 10).unwrap(); // 17 bytes > 10: emptied first
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "c\n");
        std::fs::remove_file(&path).unwrap();
    }
}
