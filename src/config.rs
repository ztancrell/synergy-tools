use crate::PORT;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};

/// The one paired PC: where to reach it and the shared secret from pairing.
#[derive(Debug)]
pub struct Peer {
    pub host: String,
    pub secret: [u8; 32],
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex<const N: usize>(s: &str) -> Option<[u8; N]> {
    let mut out = [0; N];
    if s.len() != 2 * N {
        return None;
    }
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

pub fn config_dir() -> Result<PathBuf, String> {
    dirs::config_dir().map(|d| d.join("synergy-dragndrop")).ok_or_else(|| "no config directory on this system".to_string())
}

const NOT_PAIRED: &str = "not paired yet: run `synergy-dragndrop pair` on one PC, then `synergy-dragndrop pair <host> <code>` on the other";

pub fn load_peer_from(dir: &Path) -> Result<Peer, String> {
    let path = dir.join("peer");
    let text = std::fs::read_to_string(&path).map_err(|_| format!("{NOT_PAIRED} ({} missing)", path.display()))?;
    match text.split_whitespace().collect::<Vec<_>>()[..] {
        [host, secret] => match unhex(secret) {
            Some(secret) => Ok(Peer { host: host.to_string(), secret }),
            None => Err(format!("{} is damaged: run `synergy-dragndrop pair` again", path.display())),
        },
        _ => Err(format!("{} is damaged: run `synergy-dragndrop pair` again", path.display())),
    }
}

pub fn save_peer_to(dir: &Path, p: &Peer) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let tmp = dir.join("peer.tmp");
    let write = || -> std::io::Result<()> {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        std::io::Write::write_all(&mut opts.open(&tmp)?, format!("{} {}\n", p.host, hex(&p.secret)).as_bytes())?;
        std::fs::rename(&tmp, dir.join("peer"))
    };
    write().map_err(|e| format!("cannot save {}: {e}", dir.join("peer").display()))
}

pub fn load_peer() -> Result<Peer, String> {
    load_peer_from(&config_dir()?)
}

pub fn save_peer(p: &Peer) -> Result<(), String> {
    save_peer_to(&config_dir()?, p)
}

/// Forget the pairing. Ok(false) if there was none.
pub fn remove_peer() -> Result<bool, String> {
    let path = config_dir()?.join("peer");
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("cannot remove {}: {e}", path.display())),
    }
}

/// This PC's LAN address as the other PC would reach it, else the PC name.
pub fn local_address() -> String {
    // ponytail: picks the interface of the default route; a multi-homed PC may need the user to substitute.
    let probe = || {
        let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
        s.connect("192.0.2.1:9").ok()?; // no packet is sent
        Some(s.local_addr().ok()?.ip().to_string())
    };
    probe().unwrap_or_else(this_pc_name)
}

/// Name shown in the other PC's notifications.
pub fn this_pc_name() -> String {
    #[cfg(windows)]
    let name = std::env::var("COMPUTERNAME").ok();
    #[cfg(not(windows))]
    let name = std::fs::read_to_string("/proc/sys/kernel/hostname").ok().map(|s| s.trim().to_string());
    name.filter(|n| !n.is_empty()).unwrap_or_else(|| "other PC".to_string())
}

/// "host" or "host:port" (default PORT) to an IPv4 address.
pub fn resolve(target: &str) -> Result<SocketAddr, String> {
    if target.contains(':') { target.to_socket_addrs() } else { (target, PORT).to_socket_addrs() }
        .map_err(|e| format!("bad target {target:?}: {e}"))?
        .find(SocketAddr::is_ipv4)
        .ok_or(format!("{target:?} has no IPv4 address"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let mut r = [0u8; 8];
        getrandom::fill(&mut r).unwrap();
        std::env::temp_dir().join(format!("sdd-test-{}", hex(&r)))
    }

    #[test]
    fn peer_round_trip() {
        let d = tmp();
        save_peer_to(&d, &Peer { host: "192.168.1.5".into(), secret: [7; 32] }).unwrap();
        let p = load_peer_from(&d).unwrap();
        assert_eq!((p.host.as_str(), p.secret), ("192.168.1.5", [7; 32]));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn missing_peer_says_how_to_pair() {
        assert!(load_peer_from(&tmp()).unwrap_err().contains("synergy-dragndrop pair"));
    }

    #[test]
    fn damaged_peer_file_rejected() {
        let d = tmp();
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("peer"), "host zz\n").unwrap();
        assert!(load_peer_from(&d).unwrap_err().contains("damaged"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn peer_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp();
        save_peer_to(&d, &Peer { host: "h".into(), secret: [1; 32] }).unwrap();
        let mode = std::fs::metadata(d.join("peer")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn resolve_default_port() {
        assert_eq!(resolve("127.0.0.1").unwrap().port(), 24851);
    }
}
