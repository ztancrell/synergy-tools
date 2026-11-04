//! synergy-audio step 1: stream system audio from one machine to another over UDP.
//!
//! Wire format: one UDP datagram = [seq: u64 LE][i16 LE interleaved stereo samples].
//! 48 kHz, 240 frames (5 ms) per packet = 960 B audio + 8 B header.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig, SupportedStreamConfig};
use std::collections::VecDeque;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PORT: u16 = 24850;
const RATE: u32 = 48_000;
const FRAMES_PER_PACKET: usize = 240; // 5 ms at 48 kHz
const SAMPLES_PER_PACKET: usize = FRAMES_PER_PACKET * 2; // stereo
const SAMPLES_PER_MS: usize = RATE as usize / 1000 * 2;
const DEFAULT_BUFFER_MS: usize = 20;
const STATS_EVERY: Duration = Duration::from_secs(5);
const SEND_QUEUE: usize = 20; // packets between capture callback and sender thread (~100 ms)

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let res = match args.as_slice() {
        ["send", target] => send(target),
        ["receive"] => receive(DEFAULT_BUFFER_MS),
        ["receive", "--buffer-ms", n] => match n.parse() {
            Ok(n) if n > 0 => receive(n),
            _ => Err(format!("--buffer-ms needs a positive integer, got {n:?}")),
        },
        _ => Err(format!(
            "usage:\n  synergy-audio send <host[:port]>   (default port {PORT})\n  \
             synergy-audio receive [--buffer-ms N]   (default {DEFAULT_BUFFER_MS})"
        )),
    };
    if let Err(e) = res {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

// ---------- packet ----------

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
                 Linux/PipeWire: default.clock.rate = 48000) and retry.",
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

    // On Windows (WASAPI) an input stream on an output device is loopback capture.
    let device = default_output()?;
    let sup = config_48k(&device)?;
    let config: StreamConfig = sup.config();
    eprintln!(
        "send: capturing '{device}' ({} ch, {:?}, {} Hz) -> {addr}",
        config.channels, sup.sample_format(), config.sample_rate
    );
    if config.channels > 2 {
        eprintln!("warning: {} channels, sending only the first two (front L/R)", config.channels);
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

    let mut prev = 0;
    loop {
        std::thread::sleep(STATS_EVERY);
        let n = sent.load(Relaxed);
        eprintln!(
            "send: {:.0} packets/s, queue-full dropped {}, send errors {}",
            (n - prev) as f64 / STATS_EVERY.as_secs_f64(),
            dropped.load(Relaxed),
            errors.load(Relaxed)
        );
        prev = n;
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
            |e| eprintln!("capture stream error: {e}"),
            None,
        )
        .map_err(|e| format!("can't open capture stream: {e}"))
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
            match decode(&buf[..n]) {
                None => eprintln!("receive: ignoring malformed {n}-byte packet from {from}"),
                Some((seq, _)) if !accept(last, seq) => late += 1,
                Some((seq, samples)) => {
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
    fn late_and_duplicate_dropped() {
        assert!(accept(None, 0));
        assert!(accept(Some(5), 6));
        assert!(!accept(Some(5), 5)); // duplicate
        assert!(!accept(Some(5), 3)); // late
        assert!(accept(Some(50_000), 0)); // sender restarted
    }
}
