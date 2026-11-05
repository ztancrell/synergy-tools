//! synergy-tools audio: stream system audio from one machine to another over UDP, encrypted.
//!
//! Pairing is shared with file transfer (`synergy-tools pair`, see `pair.rs`): it gives both
//! sides a partner with a 32-byte secret and an 8-byte id, used here as peer id and PSK.
//! Session: Noise NNpsk0 with that secret as PSK. `HELLO` + peer id + Noise message 1
//! (sender), `HELLO_REPLY` + message 2 (receiver). After that every datagram, both ways, is
//! [b'D'][nonce: u64 LE][ChaChaPoly(payload) + 16-byte tag]:
//! audio (sender -> receiver, nonce = seq, payload = 240 stereo i16 LE frames = 5 ms at
//! 48 kHz, 985 B in all), keepalive (sender -> receiver every 500 ms, nonce with `KA_BIT` set)
//! and ack (receiver -> sender in reply, its own nonce counter). Keepalive and ack payload is
//! the 9-byte stop control `Ctl` ([stopped 0/1][epoch u64 LE]): each side merges the other's
//! (newer epoch wins, a tie goes to stopped) and answers with its own. While stopped the sender
//! drops captured audio, the receiver discards what arrives, and nothing is muted.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig, SupportedStreamConfig};
use snow::{HandshakeState, StatelessTransportState};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use crate::config;
use std::net::{SocketAddr, UdpSocket};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{sync_channel, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const PORT: u16 = 24850;
const RATE: u32 = 48_000;
const FRAMES_PER_PACKET: usize = 240; // 5 ms at 48 kHz
const SAMPLES_PER_PACKET: usize = FRAMES_PER_PACKET * 2; // stereo
const SAMPLES_PER_MS: usize = RATE as usize / 1000 * 2;
pub const DEFAULT_BUFFER_MS: usize = 20;
const STATS_EVERY: Duration = Duration::from_secs(5);
const TICK: Duration = Duration::from_millis(100); // how often loops check `quit`
const RELOAD_EVERY: Duration = Duration::from_secs(1); // unknown peer id: reload peers at most this often
const SEND_QUEUE: usize = 20; // packets between capture callback and sender thread (~100 ms)
const LOG_MAX: u64 = 1 << 20; // --log file is emptied when it grows past 1 MiB
const RETRY_EVERY: Duration = Duration::from_secs(10);
const KEEPALIVE_EVERY: Duration = Duration::from_millis(500);
const ACK_FRESH: Duration = Duration::from_millis(1500); // newer ack = receiver is playing us
const REHANDSHAKE: Duration = Duration::from_secs(3); // no ack for this long: new handshake
const NOISE: &str = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s";
const HELLO: &[u8] = b"SAh1";
const HELLO_REPLY: &[u8] = b"SAh2";
const KA_BIT: u64 = 1 << 63; // keepalive nonces; audio seqs never get that high

/// Status line to the --log file if one is open, else stderr (see `say_line`).
macro_rules! say {
    ($($t:tt)*) => { say_line(format!($($t)*)) };
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
pub fn run_send_logged(target: &str, path: &str) -> Result<(), String> {
    LOG.set(Mutex::new(open_log(path)?)).unwrap();
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    say!("--- synergy-tools audio send started (unix time {secs}) ---");
    // At login the audio device or network may not be ready yet; Task Scheduler does not
    // restart a task whose process exits with an error, so retry here instead.
    loop {
        if let Err(e) = run_send(target) {
            say!("error: {e} (retrying in {}s)", RETRY_EVERY.as_secs());
        }
        std::thread::sleep(RETRY_EVERY);
    }
}

// ---------- stop control and shared state ----------

/// Stop control, carried by every keepalive and ack. Last writer wins by `epoch`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Ctl {
    pub stopped: bool,
    pub epoch: u64,
}

impl Ctl {
    /// [stopped as u8][epoch u64 LE]
    pub fn encode(self) -> [u8; 9] {
        let mut b = [0; 9];
        b[0] = self.stopped as u8;
        b[1..].copy_from_slice(&self.epoch.to_le_bytes());
        b
    }

    /// Exactly 9 bytes, first byte 0 or 1.
    pub fn decode(b: &[u8]) -> Option<Ctl> {
        let b: &[u8; 9] = b.try_into().ok()?;
        (b[0] <= 1).then(|| Ctl { stopped: b[0] == 1, epoch: u64::from_le_bytes(b[1..].try_into().unwrap()) })
    }

    /// Adopt `remote` if it is newer, or the same epoch and stopped while we are not.
    /// True if that changed anything.
    pub fn merge(&mut self, remote: Ctl) -> bool {
        let take = remote.epoch > self.epoch || (remote.epoch == self.epoch && remote.stopped && !self.stopped);
        if take {
            *self = remote;
        }
        take
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Waiting,
    Streaming([u8; 8]), // partner's peer id
    Stopped,
    #[allow(dead_code)] // set by the tray when send/receive returns an error
    Error(String),
}

/// State shared between the engine (`send`/`receive`) and whoever drives it.
pub struct Shared {
    pub ctl: Mutex<Ctl>,
    pub auto_mute: AtomicBool, // Windows sender: mute the speakers while streaming
    #[allow(dead_code)] // read by the tray; the CLI prints its own stats lines
    pub status: Mutex<Status>,
}

impl Shared {
    pub fn new(auto_mute: bool) -> Arc<Shared> {
        Arc::new(Shared { ctl: Mutex::default(), auto_mute: AtomicBool::new(auto_mute), status: Mutex::new(Status::Waiting) })
    }

    #[allow(dead_code)] // called by the tray's Stop item
    pub fn set_stopped(&self, on: bool) {
        let mut c = self.ctl.lock().unwrap();
        c.stopped = on;
        c.epoch = c.epoch.saturating_add(1);
    }
}

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
    Stopped,  // authentic audio while stopped: discarded
    Rejected, // unknown peer, bad MAC, malformed
}

/// Receiver session state. A new handshake goes into `next` and replaces `cur` only once a
/// packet authenticates under it, so a replayed old hello can't break a running stream.
struct Rx {
    peers: HashMap<[u8; 8], [u8; 32]>,
    load_peers: LoadPeers,
    reloaded: Option<Instant>, // last time an unknown peer id made us call `load_peers`
    cur: Option<Sess>,
    next: Option<Sess>,
    promoted: u64, // sessions started, for the log line
}

type LoadPeers = fn() -> Result<HashMap<[u8; 8], [u8; 32]>, String>;

impl Rx {
    fn new(load_peers: LoadPeers) -> Rx {
        Rx { peers: HashMap::new(), load_peers, reloaded: None, cur: None, next: None, promoted: 0 }
    }

    /// One datagram. Keepalives merge the sender's `Ctl` into `ctl`; acks carry ours.
    fn handle(&mut self, buf: &[u8], ctl: &mut Ctl) -> In {
        if let Some(rest) = buf.strip_prefix(HELLO) {
            let Some((peer, msg)) = rest.split_first_chunk::<8>() else { return In::Rejected };
            // A sender paired since we loaded: reload, at most once per second (hellos are cheap to fake).
            if !self.peers.contains_key(peer) && self.reloaded.is_none_or(|t| t.elapsed() >= RELOAD_EVERY) {
                self.reloaded = Some(Instant::now());
                if let Ok(p) = (self.load_peers)() {
                    self.peers = p;
                }
            }
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
            let Some(remote) = Ctl::decode(&payload) else { return In::Rejected };
            s.last_ka = Some(nonce);
            ctl.merge(remote);
            s.acks += 1;
            return In::Send(seal(&s.t, s.acks - 1, &ctl.encode()));
        }
        if !accept(s.last_audio, nonce) {
            return In::Late;
        }
        s.last_audio = Some(nonce);
        if ctl.stopped {
            return In::Stopped;
        }
        pcm_samples(&payload).map_or(In::Rejected, In::Audio)
    }
}

/// Accept a packet unless it is late, a duplicate or a replay. A restarted sender makes a
/// new session (new keys, counters start over), so within one session counters only grow.
fn accept(last: Option<u64>, seq: u64) -> bool {
    last.is_none_or(|last| seq > last)
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

/// Paired PCs as peer id -> secret, for the receiver. Errors if there are none.
pub fn peers() -> Result<HashMap<[u8; 8], [u8; 32]>, String> {
    peers_from(&config::config_dir()?)
}

fn peers_from(dir: &Path) -> Result<HashMap<[u8; 8], [u8; 32]>, String> {
    let ps = config::load_partners_from(dir)?;
    if ps.is_empty() {
        return Err("not paired yet: run `synergy-tools pair` on one machine, then `synergy-tools pair <host> <code>` on the other".into());
    }
    Ok(ps.into_iter().map(|p| (p.id, p.secret)).collect())
}

pub fn bind_receiver(port: u16) -> Result<UdpSocket, String> {
    UdpSocket::bind(("0.0.0.0", port)).map_err(|e| {
        format!(
            "can't bind UDP {port}: {e}. Is another synergy-tools or the old synergy-audio receiver running? \
             Stop it (old one: systemctl --user disable --now synergy-audio)"
        )
    })
}

/// CLI `audio send`: the partner named by `target`'s host part, auto-mute on, runs until killed.
pub fn run_send(target: &str) -> Result<(), String> {
    let partners = config::load_partners()?;
    let host = target.rsplit_once(':').map_or(target, |(h, _)| h);
    let p = config::find(&partners, host)?;
    let addr = config::resolve(target, PORT)?;
    say!("send: paired with {}", p.name);
    send(addr, p.id, p.secret, Shared::new(true), Arc::new(AtomicBool::new(false)))
}

/// Capture the default output device and stream it to `addr` until `quit` is set; then stop
/// the stream and threads, unmute if we muted, and return `Ok(())` (within about 1 s).
pub fn send(addr: SocketAddr, peer: [u8; 8], psk: [u8; 32], sh: Arc<Shared>, quit: Arc<AtomicBool>) -> Result<(), String> {
    let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    sock.connect(addr).map_err(|e| e.to_string())?;
    // Same socket for keepalives, so the receiver's acks come back to it.
    let ka_sock = sock.try_clone().map_err(|e| e.to_string())?;

    // On Windows (WASAPI) and macOS 14.6+ (CoreAudio) an input stream on an output device is
    // loopback capture.
    let device = default_output()?;
    let sup = config_48k(&device)?;
    let config: StreamConfig = sup.config();
    say!("send: capturing '{device}' ({} ch, {:?}, {} Hz) -> {addr}", config.channels, sup.sample_format(), config.sample_rate);
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
    let sender = {
        let (sent, errors, session, sh, quit) = (sent.clone(), errors.clone(), session.clone(), sh.clone(), quit.clone());
        std::thread::spawn(move || {
            // Ends when the capture stream (and with it `tx`) is gone, or on quit.
            while !quit.load(Relaxed) {
                let (seq, pcm) = match rx.recv_timeout(TICK) {
                    Ok(p) => p,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => break,
                };
                if sh.ctl.lock().unwrap().stopped {
                    continue;
                }
                let Some(pkt) = session.lock().unwrap().as_ref().map(|t| seal(t, seq, &pcm)) else { continue };
                // Errors are expected while the receiver isn't up yet; count, don't stop.
                match sock.send(&pkt) {
                    Ok(_) => sent.fetch_add(1, Relaxed),
                    Err(_) => errors.fetch_add(1, Relaxed),
                };
            }
        })
    };
    let stream = match sup.sample_format() {
        SampleFormat::F32 => build_capture::<f32>(&device, &config, tx, dropped.clone()),
        SampleFormat::I16 => build_capture::<i16>(&device, &config, tx, dropped.clone()),
        SampleFormat::I32 => build_capture::<i32>(&device, &config, tx, dropped.clone()),
        f => return Err(format!("unsupported sample format {f:?}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;

    // Spawned last: nothing below returns early, so run_send_logged's retries never stack threads.
    let link = Arc::new(Mutex::new(Link { last_ack: None, mute: String::new(), rejected: 0 }));
    let ka = {
        let (link, quit) = (link.clone(), quit.clone());
        std::thread::spawn(move || keepalive(ka_sock, link, session, peer, psk, sh, quit))
    };

    let mut prev = 0;
    let mut next_stats = Instant::now() + STATS_EVERY;
    while !quit.load(Relaxed) {
        std::thread::sleep(TICK);
        if Instant::now() < next_stats {
            continue;
        }
        next_stats += STATS_EVERY;
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
    drop(stream);
    let _ = ka.join(); // unmutes if it muted
    let _ = sender.join();
    Ok(())
}

/// What the keepalive thread knows, for the status line.
struct Link {
    last_ack: Option<Instant>, // last authentic ack
    mute: String,
    rejected: u64, // datagrams that failed to authenticate, or replays
}

/// Every 500 ms: (re)start the handshake if needed, send a keepalive carrying our `Ctl`,
/// collect acks (merging the receiver's `Ctl`), then update the status and mute/unmute the
/// speakers. Runs whether or not audio is playing (loopback capture sends nothing during
/// silence). On `quit`: unmutes if it muted, and returns.
fn keepalive(
    sock: UdpSocket,
    link: Arc<Mutex<Link>>,
    session: Arc<Mutex<Option<StatelessTransportState>>>,
    peer: [u8; 8],
    psk: [u8; 32],
    sh: Arc<Shared>,
    quit: Arc<AtomicBool>,
) {
    mute::restore(); // a previous run that was killed may have left the speakers muted
    mute::on_ctrl_c_restore();
    let mut muted_by_us = false;
    let mut buf = [0u8; 2048];
    let (mut pending, mut heard, mut last_ack_n, mut ka_n) = (None, None::<Instant>, None, 0u64);
    while !quit.load(Relaxed) {
        // No session yet, or no ack for a while (receiver restarted?): new handshake.
        // Errors (receiver down) just mean no answer.
        if heard.is_none_or(|t| t.elapsed() > REHANDSHAKE) {
            let (hs, msg) = hello(&peer, &psk);
            pending = Some(hs);
            let _ = sock.send(&msg);
        }
        if let Some(t) = &*session.lock().unwrap() {
            let ctl = *sh.ctl.lock().unwrap();
            let _ = sock.send(&seal(t, KA_BIT | ka_n, &ctl.encode()));
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
                    match ack.and_then(|(n, payload)| Some((n, Ctl::decode(&payload)?))) {
                        Some((n, remote)) if accept(last_ack_n, n) => {
                            last_ack_n = Some(n);
                            heard = Some(Instant::now());
                            l.last_ack = heard;
                            sh.ctl.lock().unwrap().merge(remote);
                        }
                        _ => l.rejected += 1,
                    }
                }
                // Timeout, or e.g. "connection reset" from an ICMP port-unreachable: wait out the tick.
                Err(_) => std::thread::sleep(deadline.saturating_duration_since(Instant::now())),
            }
        }
        let fresh = link.lock().unwrap().last_ack.is_some_and(|t| t.elapsed() < ACK_FRESH);
        let stopped = sh.ctl.lock().unwrap().stopped;
        *sh.status.lock().unwrap() = match (stopped, fresh) {
            (true, _) => Status::Stopped,
            (false, true) => Status::Streaming(peer),
            (false, false) => Status::Waiting,
        };
        let state = mute::update(fresh && !stopped && sh.auto_mute.load(Relaxed), &mut muted_by_us);
        link.lock().unwrap().mute = state;
    }
    mute::update(false, &mut muted_by_us); // unmute if we muted
}

#[derive(Debug, PartialEq)]
enum MuteAction {
    Mute,
    Unmute,
    Nothing,
}

/// Mute while we `want` it (receiver acks, not stopped, auto-mute on), unmute when we no
/// longer do, and never touch a mute we didn't set.
fn mute_action(want: bool, muted: bool, muted_by_us: bool) -> MuteAction {
    match (want, muted, muted_by_us) {
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
pub(crate) mod mute {
    use super::{mute_action, MuteAction};
    use std::path::PathBuf;
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

    /// Exists exactly while we have the speakers muted, so a killed sender can be undone.
    fn marker() -> PathBuf {
        let base = std::env::var_os("LOCALAPPDATA").map_or_else(std::env::temp_dir, PathBuf::from);
        base.join("synergy-tools").join("muted-by-us")
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
    pub fn update(want: bool, by_us: &mut bool) -> String {
        let r = (|| -> windows::core::Result<&str> {
            let muted = unsafe { endpoint()?.GetMute()?.as_bool() };
            if !muted && *by_us {
                *by_us = false; // the user unmuted it; it's no longer ours to undo
                let _ = std::fs::remove_file(marker());
            }
            Ok(match mute_action(want, muted, *by_us) {
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
pub(crate) mod mute {
    use super::{mute_action, MuteAction};

    pub fn restore() {}
    pub fn on_ctrl_c_restore() {}
    pub fn update(want: bool, would_be_muted: &mut bool) -> String {
        match mute_action(want, *would_be_muted, *would_be_muted) {
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

/// CLI `audio receive`: needs at least one partner, binds UDP `PORT`, runs until killed.
pub fn run_receive(buffer_ms: usize) -> Result<(), String> {
    peers()?;
    receive(bind_receiver(PORT)?, buffer_ms, peers, Shared::new(true), Arc::new(AtomicBool::new(false)))
}

/// Play what paired senders stream to `sock` until `quit` is set; returns within about 1 s.
/// `load_peers` is called at start and again (at most once a second) for an unknown peer id.
pub fn receive(sock: UdpSocket, buffer_ms: usize, load_peers: LoadPeers, sh: Arc<Shared>, quit: Arc<AtomicBool>) -> Result<(), String> {
    let target = buffer_ms * SAMPLES_PER_MS;
    // Above this the sender clock is running ahead (or a burst arrived): drop back to target.
    let max = target + target.max(10 * SAMPLES_PER_MS);

    let mut rx = Rx::new(load_peers);
    rx.peers = load_peers().unwrap_or_default();
    sock.set_read_timeout(Some(TICK)).map_err(|e| e.to_string())?;

    let device = default_output()?;
    let sup = config_48k(&device)?;
    let mut config: StreamConfig = sup.config();
    // Ask for ~5 ms device periods when the backend reports a range.
    if let cpal::SupportedBufferSize::Range { min, max } = *sup.buffer_size() {
        config.buffer_size = cpal::BufferSize::Fixed((FRAMES_PER_PACKET as u32).clamp(min, max));
    }
    let local = sock.local_addr().map_err(|e| e.to_string())?;
    eprintln!(
        "receive: UDP {local} -> '{device}' ({} ch, {:?}, {} Hz, device buffer {:?}), jitter buffer {buffer_ms} ms, {} paired sender(s)",
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
    let mut last_audio: Option<Instant> = None; // last authentic audio packet, for the status
    while !quit.load(Relaxed) {
        if let Ok((n, from)) = sock.recv_from(&mut buf) {
            let sessions = rx.promoted;
            let got = rx.handle(&buf[..n], &mut sh.ctl.lock().unwrap());
            match got {
                In::Send(reply) => {
                    let _ = sock.send_to(&reply, from); // hello reply, or ack (the sender mutes its speakers)
                }
                In::Late => late += 1,
                In::Rejected => rejected += 1,
                In::Stopped => {}
                In::Audio(samples) => {
                    last_audio = Some(Instant::now());
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
                eprintln!("receive: encrypted session with sender {} at {from}", crate::config::hex(&rx.cur.as_ref().unwrap().peer));
            }
        }
        let stopped = sh.ctl.lock().unwrap().stopped;
        if stopped {
            jb.lock().unwrap().buf.clear();
        }
        let streaming = last_audio.is_some_and(|t| t.elapsed() < ACK_FRESH);
        *sh.status.lock().unwrap() = match (stopped, streaming, &rx.cur) {
            (true, _, _) => Status::Stopped,
            (false, true, Some(s)) => Status::Streaming(s.peer),
            _ => Status::Waiting,
        };
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
    Ok(())
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

    #[test]
    fn peers_come_from_partner_list() {
        let mut r = [0u8; 8];
        getrandom::fill(&mut r).unwrap();
        let d = std::env::temp_dir().join(format!("sdd-audio-{}", config::hex(&r)));
        assert!(peers_from(&d).unwrap_err().contains("not paired yet"));
        for (n, b) in [("a", 1u8), ("b", 2)] {
            let p = config::Partner { id: [b; 8], host: "h".into(), secret: [b + 10; 32], os: "linux".into(), name: n.into() };
            config::add_partner_to(&d, p).unwrap();
        }
        let m = peers_from(&d).unwrap();
        assert_eq!((m.len(), m[&[1; 8]], m[&[2; 8]]), (2, [11; 32], [12; 32]));
        std::fs::remove_dir_all(&d).unwrap();
    }

    const PEER: [u8; 8] = [1; 8];
    const PSK: [u8; 32] = [7; 32];

    fn one_peer() -> Result<HashMap<[u8; 8], [u8; 32]>, String> {
        Ok(HashMap::from([(PEER, PSK)]))
    }

    /// Rx with `PEER` paired and its handshake done: (rx, sender transport, the hello).
    fn rx_session() -> (Rx, StatelessTransportState, Vec<u8>) {
        let mut rx = Rx::new(one_peer);
        rx.peers = one_peer().unwrap();
        let (hs, hello_msg) = hello(&PEER, &PSK);
        let In::Send(reply) = rx.handle(&hello_msg, &mut Ctl::default()) else { panic!("no hello reply") };
        (rx, finish_hello(hs, reply.strip_prefix(HELLO_REPLY).unwrap()).unwrap(), hello_msg)
    }

    #[test]
    fn session_round_trip_tamper_replay_unpaired() {
        let (peer, psk) = (PEER, PSK);
        let (mut rx, tx, hello_msg) = rx_session();
        let ctl = &mut Ctl::default();
        let samples = vec![1i16, -2, 3, -4];
        let pkt = seal(&tx, 0, &pcm_bytes(&samples));
        assert_eq!(rx.handle(&pkt, ctl), In::Audio(samples.clone()));
        assert_eq!(rx.promoted, 1);
        assert_eq!(rx.handle(&pkt, ctl), In::Late); // replay
        let mut bad = seal(&tx, 1, &pcm_bytes(&samples));
        bad[12] ^= 1;
        assert_eq!(rx.handle(&bad, ctl), In::Rejected); // tampered
        // Keepalive -> ack the sender can authenticate; a replayed keepalive gets no ack.
        let ka = seal(&tx, KA_BIT, &Ctl::default().encode());
        let In::Send(ack) = rx.handle(&ka, ctl) else { panic!("no ack") };
        assert_eq!(open(&tx, &ack), Some((0, Ctl::default().encode().to_vec())));
        assert_eq!(rx.handle(&ka, ctl), In::Late);
        // A keepalive without a 9-byte Ctl is rejected.
        assert_eq!(rx.handle(&seal(&tx, KA_BIT | 1, &[]), ctl), In::Rejected);
        // Unpaired peer id, and a paired id with the wrong secret.
        assert_eq!(rx.handle(&hello(&[9; 8], &psk).1, ctl), In::Rejected);
        assert_eq!(rx.handle(&hello(&peer, &[8; 32]).1, ctl), In::Rejected);
        assert_eq!(rx.handle(b"SAka", ctl), In::Rejected); // old plaintext keepalive
        // A replayed old hello is answered but doesn't break the running session.
        assert!(matches!(rx.handle(&hello_msg, ctl), In::Send(_)));
        assert_eq!(rx.handle(&seal(&tx, 2, &pcm_bytes(&samples)), ctl), In::Audio(samples));
        assert_eq!(rx.promoted, 1);
    }

    #[test]
    fn ctl_encode_decode() {
        let c = Ctl { stopped: true, epoch: 0x0102_0304_0506_0708 };
        assert_eq!(Ctl::decode(&c.encode()), Some(c));
        assert_eq!(c.encode(), [1, 8, 7, 6, 5, 4, 3, 2, 1]);
        assert_eq!(Ctl::decode(&[0; 8]), None);
        assert_eq!(Ctl::decode(&[0; 10]), None);
        assert_eq!(Ctl::decode(&[2, 0, 0, 0, 0, 0, 0, 0, 0]), None);
    }

    #[test]
    fn ctl_merge_newer_wins() {
        let mut c = Ctl { stopped: true, epoch: 3 };
        assert!(!c.merge(Ctl { stopped: false, epoch: 2 })); // older: ignored
        assert_eq!(c, Ctl { stopped: true, epoch: 3 });
        assert!(c.merge(Ctl { stopped: false, epoch: 4 })); // newer resume wins
        assert_eq!(c, Ctl { stopped: false, epoch: 4 });
        assert!(!c.merge(c)); // same: no change
    }

    #[test]
    fn ctl_tie_stop_wins() {
        let mut c = Ctl { stopped: false, epoch: 5 };
        assert!(c.merge(Ctl { stopped: true, epoch: 5 }));
        assert_eq!(c, Ctl { stopped: true, epoch: 5 });
        assert!(!c.merge(Ctl { stopped: false, epoch: 5 })); // tie: stays stopped
        assert!(c.stopped);
    }

    #[test]
    fn stop_from_sender_reaches_receiver() {
        let (mut rx, tx, _) = rx_session();
        let mut ctl = Ctl::default();
        let stop = Ctl { stopped: true, epoch: 1 };
        let In::Send(ack) = rx.handle(&seal(&tx, KA_BIT, &stop.encode()), &mut ctl) else { panic!("no ack") };
        assert_eq!(ctl, stop);
        let (_, payload) = open(&tx, &ack).unwrap();
        assert_eq!(Ctl::decode(&payload), Some(stop));
    }

    #[test]
    fn stopped_receiver_drops_audio() {
        let (mut rx, tx, _) = rx_session();
        let mut ctl = Ctl { stopped: true, epoch: 1 };
        let r = rx.handle(&seal(&tx, 0, &pcm_bytes(&[1, 2, 3, 4])), &mut ctl);
        assert!(!matches!(r, In::Audio(_)), "{r:?}");
    }

    #[test]
    fn unknown_id_reloads_peers_once_per_second() {
        static LOADS: AtomicU64 = AtomicU64::new(0);
        fn counting() -> Result<HashMap<[u8; 8], [u8; 32]>, String> {
            LOADS.fetch_add(1, Relaxed);
            Ok(HashMap::new())
        }
        let mut rx = Rx::new(counting);
        let ctl = &mut Ctl::default();
        assert_eq!(rx.handle(&hello(&[9; 8], &[7; 32]).1, ctl), In::Rejected);
        assert_eq!(rx.handle(&hello(&[9; 8], &[7; 32]).1, ctl), In::Rejected);
        assert_eq!(LOADS.load(Relaxed), 1);
    }

    #[test]
    fn keepalive_stops_on_quit() {
        // A socket that reads but never answers: no receiver.
        let silent = UdpSocket::bind("127.0.0.1:0").unwrap();
        silent.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        sock.connect(silent.local_addr().unwrap()).unwrap();
        let link = Arc::new(Mutex::new(Link { last_ack: None, mute: String::new(), rejected: 0 }));
        let (sh, quit) = (Shared::new(true), Arc::new(AtomicBool::new(false)));
        let (done_tx, done) = std::sync::mpsc::channel();
        let q = quit.clone();
        std::thread::spawn(move || {
            keepalive(sock, link, Arc::new(Mutex::new(None)), [1; 8], [7; 32], sh, q);
            done_tx.send(()).unwrap();
        });
        silent.recv(&mut [0; 2048]).expect("no hello"); // the loop is running
        quit.store(true, Relaxed);
        done.recv_timeout(Duration::from_secs(2)).expect("keepalive thread didn't stop on quit");
    }

    #[test]
    fn mute_decision() {
        use MuteAction::*;
        assert_eq!(mute_action(true, false, false), Mute);
        assert_eq!(mute_action(true, true, true), Nothing); // already ours
        assert_eq!(mute_action(false, true, true), Unmute);
        assert_eq!(mute_action(false, false, false), Nothing);
        assert_eq!(mute_action(false, false, true), Nothing);
        // The user's own mute: never touched, whether we want it muted or not.
        assert_eq!(mute_action(true, true, false), Nothing);
        assert_eq!(mute_action(false, true, false), Nothing);
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
        let path = std::env::temp_dir().join(format!("synergy-tools-test-{}.log", std::process::id()));
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
