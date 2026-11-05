//! synergy-audio step 1: stream system audio from one machine to another over UDP.
//!
//! Wire format: one UDP datagram = [seq: u64 LE][i16 LE interleaved stereo samples].
//! 48 kHz, 240 frames (5 ms) per packet = 960 B audio + 8 B header.
//! Control datagrams are exactly 4 bytes, which no audio packet can be (audio is 8 + 4n):
//! `KEEPALIVE` (sender -> receiver, every 500 ms) and `ACK` (receiver -> sender, in reply).

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig, SupportedStreamConfig};
use std::collections::VecDeque;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
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
const KEEPALIVE: &[u8; 4] = b"SAka";
const ACK: &[u8; 4] = b"SAak";
const KEEPALIVE_EVERY: Duration = Duration::from_millis(500);
const ACK_FRESH: Duration = Duration::from_millis(1500); // newer ack = receiver is playing us

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
            "usage:\n  synergy-audio send <host[:port]> [--log <file>]   (default port {PORT})\n  \
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

#[derive(Debug, PartialEq)]
enum Packet {
    Audio(u64, Vec<i16>),
    Keepalive,
    Ack,
}

fn parse(buf: &[u8]) -> Option<Packet> {
    match buf {
        b if b == KEEPALIVE => Some(Packet::Keepalive),
        b if b == ACK => Some(Packet::Ack),
        b => decode(b).map(|(seq, samples)| Packet::Audio(seq, samples)),
    }
}

fn encode(seq: u64, samples: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + samples.len() * 2);
    out.extend_from_slice(&seq.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

fn decode(buf: &[u8]) -> Option<(u64, Vec<i16>)> {
    if buf.len() < 8 || !(buf.len() - 8).is_multiple_of(4) {
        return None; // not whole stereo frames
    }
    let seq = u64::from_le_bytes(buf[..8].try_into().unwrap());
    let samples = buf[8..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    Some((seq, samples))
}

/// Accept a packet unless it is late or a duplicate. A seq far behind the last one
/// means the sender restarted (seq back to 0), so accept that instead of dropping forever.
fn accept(last: Option<u64>, seq: u64) -> bool {
    match last {
        None => true,
        Some(last) => seq > last || last - seq > 1000,
    }
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

fn send(target: &str) -> Result<(), String> {
    let addr = if target.contains(':') { target.to_socket_addrs() } else { (target, PORT).to_socket_addrs() }
        .map_err(|e| format!("bad target {target:?}: {e}"))?
        .find(SocketAddr::is_ipv4)
        .ok_or(format!("{target:?} has no IPv4 address"))?;
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
        "send: capturing '{device}' ({} ch, {:?}, {} Hz) -> {addr}",
        config.channels, sup.sample_format(), config.sample_rate
    );
    if config.channels > 2 {
        say!("warning: {} channels, sending only the first two (front L/R)", config.channels);
    }

    // The capture callback must never block, so it hands packets to a sender thread.
    let (tx, rx) = sync_channel::<Vec<u8>>(SEND_QUEUE);
    let sent = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));
    let dropped = Arc::new(AtomicU64::new(0));
    {
        let (sent, errors) = (sent.clone(), errors.clone());
        std::thread::spawn(move || {
            for pkt in rx {
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
    let link = Arc::new(Mutex::new(Link { last_ack: None, mute: String::new() }));
    {
        let link = link.clone();
        std::thread::spawn(move || keepalive(ka_sock, link));
    }

    let mut prev = 0;
    loop {
        std::thread::sleep(STATS_EVERY);
        let n = sent.load(Relaxed);
        let (age, mute) = {
            let l = link.lock().unwrap();
            (l.last_ack.map(|t| t.elapsed()), l.mute.clone())
        };
        let receiver = match age {
            Some(a) if a < ACK_FRESH => format!("receiver ok (ack {:.1} s ago)", a.as_secs_f64()),
            Some(a) => format!("receiver not heard (last ack {:.0} s ago)", a.as_secs_f64()),
            None => "receiver not heard".into(),
        };
        say!(
            "send: {:.0} packets/s, queue-full dropped {}, send errors {}, {receiver}, {mute}",
            (n - prev) as f64 / STATS_EVERY.as_secs_f64(),
            dropped.load(Relaxed),
            errors.load(Relaxed)
        );
        prev = n;
    }
}

/// What the keepalive thread knows, for the status line.
struct Link {
    last_ack: Option<Instant>,
    mute: String,
}

/// Every 500 ms: send a keepalive, collect acks, then mute/unmute the speakers.
/// Runs whether or not audio is playing (loopback capture sends nothing during silence).
fn keepalive(sock: UdpSocket, link: Arc<Mutex<Link>>) {
    mute::restore(); // a previous run that was killed may have left the speakers muted
    mute::on_ctrl_c_restore();
    let mut muted_by_us = false;
    let mut buf = [0u8; 2048];
    loop {
        let _ = sock.send(KEEPALIVE); // errors (receiver down) just mean no ack
        let deadline = Instant::now() + KEEPALIVE_EVERY;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            sock.set_read_timeout(Some(left)).unwrap();
            match sock.recv(&mut buf) {
                Ok(n) if parse(&buf[..n]) == Some(Packet::Ack) => link.lock().unwrap().last_ack = Some(Instant::now()),
                Ok(_) => {}
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
    tx: SyncSender<Vec<u8>>,
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
                    if let Err(TrySendError::Full(_)) = tx.try_send(encode(seq, chunk)) {
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
        "receive: UDP 0.0.0.0:{PORT} -> '{device}' ({} ch, {:?}, {} Hz, device buffer {:?}), jitter buffer {buffer_ms} ms",
        config.channels, sup.sample_format(), config.sample_rate, config.buffer_size
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
    let (mut last, mut pkts, mut late, mut trimmed) = (None, 0u64, 0u64, 0u64);
    let mut next_stats = Instant::now() + STATS_EVERY;
    loop {
        if let Ok((n, from)) = sock.recv_from(&mut buf) {
            match parse(&buf[..n]) {
                None => eprintln!("receive: ignoring malformed {n}-byte packet from {from}"),
                Some(Packet::Keepalive) => {
                    let _ = sock.send_to(ACK, from); // tells the sender we're here (it mutes its speakers)
                }
                Some(Packet::Ack) => {}
                Some(Packet::Audio(seq, _)) if !accept(last, seq) => late += 1,
                Some(Packet::Audio(seq, samples)) => {
                    if last.is_none() {
                        eprintln!("receive: audio arriving from {from}");
                    }
                    last = Some(seq);
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
        }
        if Instant::now() >= next_stats {
            let (level, underruns) = {
                let j = jb.lock().unwrap();
                (j.buf.len() / SAMPLES_PER_MS, j.underruns)
            };
            eprintln!(
                "receive: {:.0} packets/s, buffered {level} ms, underruns {underruns}, late/dup dropped {late}, trims {trimmed}",
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
    fn packet_round_trip() {
        let samples: Vec<i16> = (0..SAMPLES_PER_PACKET as i32).map(|i| (i * 137 - 30000) as i16).collect();
        let pkt = encode(42, &samples);
        assert_eq!(pkt.len(), 8 + 960);
        assert_eq!(decode(&pkt), Some((42, samples)));
        assert_eq!(decode(&pkt[..9]), None);
    }

    #[test]
    fn packet_types() {
        assert_eq!(parse(KEEPALIVE), Some(Packet::Keepalive));
        assert_eq!(parse(ACK), Some(Packet::Ack));
        assert_eq!(parse(&encode(7, &[1, 2])), Some(Packet::Audio(7, vec![1, 2])));
        assert_eq!(parse(b"SAxx"), None); // 4 bytes but no known type, and too short for audio
        assert_eq!(parse(&[]), None);
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
        assert!(accept(Some(50_000), 0)); // sender restarted
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
