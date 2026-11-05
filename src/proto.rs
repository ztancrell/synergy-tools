//! Encrypted wire layer shared by pairing and transfers.
//!
//! Frame: `u16` big-endian length + bytes. A connection starts with one mode byte
//! (`b'P'` pairing, `b'S'` session), then frames.
//!
//! Session: `Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s`, PSK = pairing secret, prologue
//! `synergy-dragndrop v1`. The client sends handshake message 1, the server replies with
//! message 2; a wrong PSK makes the server's handshake fail. After that every frame is one
//! Noise transport message whose plaintext is a `Msg`:
//!   `B` count:u16 sender-name   batch header (client)
//!   `F` size:u64 name           file header (client)
//!   `D` bytes                   data chunk, at most `CHUNK` bytes (client)
//!   `E`                         end of file (client)
//!   `K` saved:u16               batch done (server)
//!   `X` reason                  failure, then close (server)
//! Integers are big-endian, names are UTF-8.

use std::io::{self, Read, Write};

const NOISE: &str = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s";
const PROLOGUE: &[u8] = b"synergy-dragndrop v1";
pub const CHUNK: usize = 60_000;

pub fn write_frame(w: &mut impl Write, b: &[u8]) -> io::Result<()> {
    let len = u16::try_from(b.len()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame too long"))?;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(b)?;
    w.flush()
}

pub fn read_frame(r: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut len = [0u8; 2];
    r.read_exact(&mut len)?;
    let mut buf = vec![0; u16::from_be_bytes(len) as usize];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

#[derive(Debug, PartialEq)]
pub enum Msg {
    Batch { count: u16, from: String },
    File { size: u64, name: String },
    Data(Vec<u8>),
    End,
    Done { saved: u16 },
    Fail(String),
}

impl Msg {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Msg::Batch { count, from } => [&[b'B'][..], &count.to_be_bytes(), from.as_bytes()].concat(),
            Msg::File { size, name } => [&[b'F'][..], &size.to_be_bytes(), name.as_bytes()].concat(),
            Msg::Data(d) => [&[b'D'][..], d].concat(),
            Msg::End => vec![b'E'],
            Msg::Done { saved } => [&[b'K'][..], &saved.to_be_bytes()].concat(),
            Msg::Fail(r) => [&[b'X'][..], r.as_bytes()].concat(),
        }
    }

    pub fn decode(b: &[u8]) -> Result<Msg, String> {
        let bad = || format!("malformed message ({} bytes)", b.len());
        let text = |s: &[u8]| String::from_utf8(s.to_vec()).map_err(|_| "message text is not valid UTF-8".to_string());
        let (&tag, rest) = b.split_first().ok_or_else(bad)?;
        Ok(match tag {
            b'B' if rest.len() >= 2 => Msg::Batch { count: u16::from_be_bytes([rest[0], rest[1]]), from: text(&rest[2..])? },
            b'F' if rest.len() >= 8 => {
                Msg::File { size: u64::from_be_bytes(rest[..8].try_into().unwrap()), name: text(&rest[8..])? }
            }
            b'D' => Msg::Data(rest.to_vec()),
            b'E' if rest.is_empty() => Msg::End,
            b'K' if rest.len() == 2 => Msg::Done { saved: u16::from_be_bytes([rest[0], rest[1]]) },
            b'X' => Msg::Fail(text(rest)?),
            _ => return Err(bad()),
        })
    }
}

pub struct Session<S: Read + Write> {
    s: S,
    t: snow::TransportState,
}

fn noise(psk: &[u8; 32]) -> snow::Builder<'_> {
    snow::Builder::new(NOISE.parse().unwrap()).psk(0, psk).unwrap().prologue(PROLOGUE).unwrap()
}

impl<S: Read + Write> Session<S> {
    /// Writes the mode byte `S`, then runs the handshake as initiator.
    pub fn client(mut s: S, psk: &[u8; 32]) -> Result<Self, String> {
        let mut hs = noise(psk).build_initiator().map_err(|e| e.to_string())?;
        let mut buf = [0u8; 256];
        let n = hs.write_message(&[], &mut buf).map_err(|e| e.to_string())?;
        s.write_all(b"S").map_err(|e| format!("connection failed: {e}"))?;
        write_frame(&mut s, &buf[..n]).map_err(|e| format!("handshake failed: {e}"))?;
        let reply = read_frame(&mut s).map_err(|e| format!("handshake failed: {e}"))?;
        hs.read_message(&reply, &mut buf).map_err(|_| "handshake failed: not the paired PC".to_string())?;
        Ok(Session { s, t: hs.into_transport_mode().map_err(|e| e.to_string())? })
    }

    /// Runs the handshake as responder; the caller has already consumed the mode byte.
    pub fn server(mut s: S, psk: &[u8; 32]) -> Result<Self, String> {
        let mut hs = noise(psk).build_responder().map_err(|e| e.to_string())?;
        let mut buf = [0u8; 256];
        let first = read_frame(&mut s).map_err(|e| format!("handshake failed: {e}"))?;
        hs.read_message(&first, &mut buf).map_err(|_| "handshake failed: not the paired PC".to_string())?;
        let n = hs.write_message(&[], &mut buf).map_err(|e| e.to_string())?;
        write_frame(&mut s, &buf[..n]).map_err(|e| format!("handshake failed: {e}"))?;
        Ok(Session { s, t: hs.into_transport_mode().map_err(|e| e.to_string())? })
    }

    pub fn send(&mut self, msg: &Msg) -> Result<(), String> {
        let plain = msg.encode();
        let mut buf = vec![0; plain.len() + 16];
        let n = self.t.write_message(&plain, &mut buf).map_err(|e| format!("encrypt failed: {e}"))?;
        write_frame(&mut self.s, &buf[..n]).map_err(|e| format!("send failed: {e}"))
    }

    pub fn recv(&mut self) -> Result<Msg, String> {
        let frame = read_frame(&mut self.s).map_err(|e| format!("receive failed: {e}"))?;
        let mut buf = vec![0; frame.len()];
        let n = self.t.read_message(&frame, &mut buf).map_err(|_| "receive failed: bad encrypted message".to_string())?;
        Msg::decode(&buf[..n])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    #[test]
    fn msg_round_trip() {
        for m in [
            Msg::Batch { count: 3, from: "pc".into() },
            Msg::File { size: 5, name: "a.zip".into() },
            Msg::Data(vec![1; CHUNK]),
            Msg::End,
            Msg::Done { saved: 2 },
            Msg::Fail("disk full".into()),
        ] {
            assert_eq!(Msg::decode(&m.encode()).unwrap(), m)
        }
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(Msg::decode(&[]).is_err());
        assert!(Msg::decode(b"F\x00").is_err());
        assert!(Msg::decode(b"Z").is_err());
        assert!(Msg::decode(b"B\x00").is_err());
        assert!(Msg::decode(b"K\x00").is_err());
        assert!(Msg::decode(b"Ex").is_err());
        assert!(Msg::decode(b"X\xff\xfe").is_err());
        assert!(Msg::decode(b"F\0\0\0\0\0\0\0\x01\xff").is_err());
    }

    /// Connected loopback pair: (client side, server side with the mode byte consumed).
    fn pair() -> (TcpStream, TcpStream) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let c = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        let s = l.accept().unwrap().0;
        for st in [&c, &s] {
            st.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        }
        (c, s)
    }

    /// Server side of `pair()` must consume the mode byte, as the real listener will.
    fn serve(mut s: TcpStream, psk: [u8; 32]) -> std::thread::JoinHandle<Result<Session<TcpStream>, String>> {
        std::thread::spawn(move || {
            let mut mode = [0u8; 1];
            s.read_exact(&mut mode).unwrap();
            assert_eq!(&mode, b"S");
            Session::server(s, &psk)
        })
    }

    #[test]
    fn session_same_psk_talks() {
        let (c, s) = pair();
        let h = serve(s, [7; 32]);
        let mut client = Session::client(c, &[7; 32]).unwrap();
        let mut server = h.join().unwrap().unwrap();
        let file = Msg::File { size: 5, name: "a.zip".into() };
        client.send(&file).unwrap();
        assert_eq!(server.recv().unwrap(), file);
        server.send(&Msg::Done { saved: 1 }).unwrap();
        assert_eq!(client.recv().unwrap(), Msg::Done { saved: 1 });
    }

    #[test]
    fn session_wrong_psk_fails() {
        let (c, s) = pair();
        let h = serve(s, [1; 32]);
        let client = Session::client(c, &[2; 32]);
        assert!(h.join().unwrap().is_err());
        assert!(client.is_err());
    }

    /// Records everything written through it.
    struct Tap(TcpStream, Arc<Mutex<Vec<u8>>>);
    impl Read for Tap {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            self.0.read(b)
        }
    }
    impl Write for Tap {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.1.lock().unwrap().extend_from_slice(b);
            self.0.write(b)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.0.flush()
        }
    }

    #[test]
    fn bytes_on_wire_are_encrypted() {
        let (c, s) = pair();
        let h = serve(s, [7; 32]);
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut client = Session::client(Tap(c, log.clone()), &[7; 32]).unwrap();
        let mut server = h.join().unwrap().unwrap();
        client.send(&Msg::File { size: 1, name: "secret-name.zip".into() }).unwrap();
        assert_eq!(server.recv().unwrap(), Msg::File { size: 1, name: "secret-name.zip".into() });
        let wire = log.lock().unwrap();
        assert!(wire.len() > 30 && !wire.windows(15).any(|w| w == b"secret-name.zip"));
    }
}
