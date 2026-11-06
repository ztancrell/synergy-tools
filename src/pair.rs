//! SPAKE2 pairing over a stream: an 8-digit code becomes a shared 32-byte secret (the Noise PSK),
//! and each side learns the other's name and OS.
//!
//! Wire (after the joiner's `P`): joiner → `msg_a`, `who_j`; host → `msg_b ‖ confB`, `who_h`;
//! joiner → `confA`; host → `OK` (all frames). `who` = `os + "\n" + name`, at most 1000 bytes.
//! `conf = SHA256(label ‖ key ‖ SHA256(who_j) ‖ SHA256(who_h))`: both confirmations cover both
//! `who`s, each hashed first so the boundary between them can't be shifted in transit.

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
    let (pw, a, b) = (Password::new(code), Identity::new(b"synergy-tools joiner"), Identity::new(b"synergy-tools host"));
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

/// Who a PC says it is during pairing.
#[derive(Debug)]
pub struct Who {
    pub name: String,
    pub os: String,
}

impl Who {
    pub fn this_pc() -> Who {
        Who { name: crate::config::this_pc_name(), os: std::env::consts::OS.into() }
    }

    fn encode(&self) -> Vec<u8> {
        format!("{}\n{}", self.os, self.name).into_bytes()
    }

    /// Refuses anything that could not be stored as one partners-file line:
    /// the OS must be `[A-Za-z0-9_]+`, the name non-empty without control characters.
    fn decode(b: &[u8]) -> Result<Who, String> {
        let bad = || "pairing failed: the other PC sent a bad name".to_string();
        let s = std::str::from_utf8(b).ok().filter(|_| b.len() <= 1000).ok_or_else(bad)?;
        let (os, name) = s.split_once('\n').ok_or_else(bad)?;
        let ok_os = !os.is_empty() && os.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        let ok = ok_os && !name.is_empty() && !name.chars().any(char::is_control);
        if !ok {
            return Err(bad());
        }
        Ok(Who { name: name.into(), os: os.into() })
    }
}

/// Sent before every session so the listener knows which secret to use.
pub fn partner_id(secret: &[u8; 32]) -> [u8; 8] {
    digest(&[b"synergy-tools id", secret])[..8].try_into().unwrap()
}

fn confirm(label: &[u8], key: &[u8], who_j: &[u8], who_h: &[u8]) -> [u8; 32] {
    digest(&[label, key, &digest(&[who_j]), &digest(&[who_h])])
}

/// Host side (the caller already read `b'P'`). One attempt; the secret exists only after both confirmations match.
pub fn host(s: &mut (impl Read + Write), code: &str, me: &Who) -> Result<(Who, [u8; 32]), String> {
    let msg_a = read_frame(s).map_err(io)?;
    let who_j = read_frame(s).map_err(io)?;
    let them = Who::decode(&who_j)?;
    let who_h = me.encode();
    let (sp, msg_b) = spake(code, false);
    let key = sp.finish(&msg_a).map_err(|_| "pairing failed: bad message from the other PC")?;
    let reply = [&msg_b[..], &confirm(b"synergy-tools confirm B", &key, &who_j, &who_h)].concat();
    write_frame(s, &reply).map_err(io)?;
    write_frame(s, &who_h).map_err(io)?;
    // A joiner that rejects our confirmation just disconnects, which looks the same as a wrong code.
    let conf = read_frame(s).map_err(|_| WRONG.to_string())?;
    if !same(&conf, &confirm(b"synergy-tools confirm A", &key, &who_j, &who_h)) {
        return Err(WRONG.into());
    }
    write_frame(s, b"OK").map_err(io)?;
    Ok((them, digest(&[b"synergy-tools secret", &key])))
}

/// Join side: writes `b'P'` first.
pub fn join(s: &mut (impl Read + Write), code: &str, me: &Who) -> Result<(Who, [u8; 32]), String> {
    s.write_all(b"P").map_err(io)?;
    let who_j = me.encode();
    let (sp, msg_a) = spake(code, true);
    write_frame(s, &msg_a).map_err(io)?;
    write_frame(s, &who_j).map_err(io)?;
    let reply = read_frame(s).map_err(io)?;
    let who_h = read_frame(s).map_err(io)?;
    let them = Who::decode(&who_h)?;
    let (msg_b, conf) = reply.split_at_checked(reply.len().saturating_sub(32)).ok_or("pairing failed: bad reply")?;
    let key = sp.finish(msg_b).map_err(|_| "pairing failed: bad reply from the other PC")?;
    if !same(conf, &confirm(b"synergy-tools confirm B", &key, &who_j, &who_h)) {
        return Err(WRONG.into());
    }
    write_frame(s, &confirm(b"synergy-tools confirm A", &key, &who_j, &who_h)).map_err(io)?;
    match read_frame(s) {
        Ok(ok) if ok == b"OK" => Ok((them, digest(&[b"synergy-tools secret", &key]))),
        _ => Err(WRONG.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    type Side = Result<(Who, [u8; 32]), String>;

    fn who(name: &str, os: &str) -> Who {
        Who { name: name.into(), os: os.into() }
    }

    fn pair_as(host_code: &str, join_code: &str, h_who: Who, j_who: Who) -> (Side, Side) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let hc = host_code.to_string();
        let t = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut first = [0u8; 1];
            s.read_exact(&mut first).unwrap();
            assert_eq!(first[0], b'P');
            host(&mut s, &hc, &h_who)
        });
        let mut c = TcpStream::connect(addr).unwrap();
        let j = join(&mut c, join_code, &j_who);
        drop(c);
        (t.join().unwrap(), j)
    }

    fn pair_over_loopback(host_code: &str, join_code: &str) -> (Side, Side) {
        pair_as(host_code, join_code, who("pc-h", "linux"), who("pc j", "windows"))
    }

    #[test]
    fn same_code_same_secret_and_names() {
        let (h, j) = pair_over_loopback("12345678", "12345678");
        let ((hw, hs), (jw, js)) = (h.unwrap(), j.unwrap());
        assert_eq!(hs, js);
        assert_eq!((hw.name.as_str(), hw.os.as_str()), ("pc j", "windows"));
        assert_eq!((jw.name.as_str(), jw.os.as_str()), ("pc-h", "linux"));
    }

    #[test]
    fn shifted_who_split_changes_confirmation() {
        let (j, h) = (b"linux\npc-j".to_vec(), b"windows\npc-h".to_vec());
        let (j2, h2) = ([&j[..], b"X"].concat(), [&b"X"[..], &h].concat());
        assert_ne!(confirm(b"l", b"k", &j, &h), confirm(b"l", b"k", &j2, &h2));
    }

    #[test]
    fn bad_who_refused() {
        assert!(Who::decode(b"linux\npc").is_ok());
        assert!(Who::decode(b"linux\npc\nevil").is_err());
        assert!(Who::decode(b"linux\npc\x07").is_err());
        assert!(Who::decode(b"linux\npc\r").is_err());
        assert!(Who::decode(b"\npc").is_err());
        assert!(Who::decode(b"lin/ux\npc").is_err());
        assert!(Who::decode("l\u{2003}x\npc".as_bytes()).is_err());
        assert!(Who::decode(b"\npc\nx").is_err());
    }

    #[test]
    fn oversized_who_refused() {
        let (h, j) = pair_as("12345678", "12345678", who("pc-h", "linux"), who(&"x".repeat(1000), "linux"));
        assert!(h.is_err() && j.is_err());
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
