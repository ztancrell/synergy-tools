//! SPAKE2 pairing over a stream: an 8-digit code becomes a shared 32-byte secret (the Noise PSK).

use crate::proto::{read_frame, write_frame};
use sha2::{Digest, Sha256};
use spake2::{Ed25519Group, Identity, Password, Spake2};
use std::io::{Read, Write};

const WRONG: &str = "pairing failed: wrong code (start again with a new code)";

fn digest(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

fn spake(code: &str, joiner: bool) -> (Spake2<Ed25519Group>, Vec<u8>) {
    let (pw, a, b) = (Password::new(code), Identity::new(b"synergy-dragndrop joiner"), Identity::new(b"synergy-dragndrop host"));
    if joiner { Spake2::start_a(&pw, &a, &b) } else { Spake2::start_b(&pw, &a, &b) }
}

/// Equal without stopping at the first differing byte.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// 8 random digits, uniform (values past the last whole multiple of 10^8 are redrawn).
pub fn new_code() -> String {
    loop {
        let mut b = [0u8; 4];
        getrandom::fill(&mut b).expect("no OS random number source");
        let n = u32::from_le_bytes(b);
        if n < 4_200_000_000 {
            return format!("{:08}", n % 100_000_000);
        }
    }
}

pub fn show_code(c: &str) -> String {
    format!("{}-{}", &c[..4], &c[4..])
}

pub fn parse_code(s: &str) -> Result<String, String> {
    let d: String = s.chars().filter(|c| *c != '-' && *c != ' ').collect();
    if d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit()) {
        Ok(d)
    } else {
        Err(format!("the code is 8 digits as shown by `synergy-tools pair`, like 1234-5678; got {s:?}"))
    }
}

fn io(e: std::io::Error) -> String {
    format!("pairing failed: {e}")
}

/// Host side (the caller already read `b'P'`). One attempt; the secret exists only after both confirmations match.
pub fn host(s: &mut (impl Read + Write), code: &str) -> Result<[u8; 32], String> {
    let msg_a = read_frame(s).map_err(io)?;
    let (sp, msg_b) = spake(code, false);
    let key = sp.finish(&msg_a).map_err(|_| "pairing failed: bad message from the other PC")?;
    let reply = [&msg_b[..], &digest(&[b"synergy-dragndrop confirm B", &key])].concat();
    write_frame(s, &reply).map_err(io)?;
    // A joiner that rejects our confirmation just disconnects, which looks the same as a wrong code.
    let conf = read_frame(s).map_err(|_| WRONG.to_string())?;
    if !same(&conf, &digest(&[b"synergy-dragndrop confirm A", &key])) {
        return Err(WRONG.into());
    }
    write_frame(s, b"OK").map_err(io)?;
    Ok(digest(&[b"synergy-dragndrop secret", &key]))
}

/// Join side: writes `b'P'` first.
pub fn join(s: &mut (impl Read + Write), code: &str) -> Result<[u8; 32], String> {
    s.write_all(b"P").map_err(io)?;
    let (sp, msg_a) = spake(code, true);
    write_frame(s, &msg_a).map_err(io)?;
    let reply = read_frame(s).map_err(io)?;
    let (msg_b, conf) = reply.split_at_checked(reply.len().saturating_sub(32)).ok_or("pairing failed: bad reply")?;
    let key = sp.finish(msg_b).map_err(|_| "pairing failed: bad reply from the other PC")?;
    if !same(conf, &digest(&[b"synergy-dragndrop confirm B", &key])) {
        return Err(WRONG.into());
    }
    write_frame(s, &digest(&[b"synergy-dragndrop confirm A", &key])).map_err(io)?;
    match read_frame(s) {
        Ok(ok) if ok == b"OK" => Ok(digest(&[b"synergy-dragndrop secret", &key])),
        _ => Err(WRONG.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    fn pair_over_loopback(host_code: &str, join_code: &str) -> (Result<[u8; 32], String>, Result<[u8; 32], String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let hc = host_code.to_string();
        let t = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut first = [0u8; 1];
            s.read_exact(&mut first).unwrap();
            assert_eq!(first[0], b'P');
            host(&mut s, &hc)
        });
        let mut c = TcpStream::connect(addr).unwrap();
        let j = join(&mut c, join_code);
        drop(c);
        (t.join().unwrap(), j)
    }

    #[test]
    fn same_code_same_secret() {
        let (h, j) = pair_over_loopback("12345678", "12345678");
        assert_eq!(h.unwrap(), j.unwrap());
    }

    #[test]
    fn wrong_code_fails_both_sides() {
        let (h, j) = pair_over_loopback("12345678", "87654321");
        assert!(h.unwrap_err().contains("wrong code"));
        assert!(j.unwrap_err().contains("wrong code"));
    }

    #[test]
    fn code_format() {
        assert_eq!(show_code("12345678"), "1234-5678");
        assert_eq!(parse_code("1234-5678").unwrap(), "12345678");
        assert_eq!(parse_code("1234 5678").unwrap(), "12345678");
        assert!(parse_code("1234").is_err());
        assert!(parse_code("abcd-efgh").is_err());
    }

    #[test]
    fn new_code_is_8_digits() {
        let c = new_code();
        assert!(parse_code(&c).is_ok() && c.len() == 8);
    }
}
