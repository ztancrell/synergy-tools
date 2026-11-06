use std::net::{SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};

/// A paired PC: its partner id (from the secret), address, shared secret, OS and name.
#[derive(Clone, PartialEq)]
pub struct Partner {
    pub id: [u8; 8],
    pub host: String,
    pub secret: [u8; 32],
    pub os: String,
    pub name: String,
}

impl std::fmt::Debug for Partner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Partner")
            .field("id", &hex(&self.id))
            .field("host", &self.host)
            .field("secret", &"<redacted>")
            .field("os", &self.os)
            .field("name", &self.name)
            .finish()
    }
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
    dirs::config_dir().map(|d| d.join("synergy-tools")).ok_or_else(|| "no config directory on this system".to_string())
}

pub const NOT_PAIRED: &str =
    "not paired yet: run `synergy-tools pair` on one PC, then `synergy-tools pair <host> <code>` on the other";

/// One line per partner: `<id hex16> <host> <secret hex64> <os> <name>`; the name is the rest of the line.
pub fn load_partners_from(dir: &Path) -> Result<Vec<Partner>, String> {
    let path = dir.join("partners");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let parse = |line: &str| -> Option<Partner> {
        let mut f = line.splitn(5, ' ');
        let (id, host, secret, os, name) = (f.next()?, f.next()?, f.next()?, f.next()?, f.next()?);
        Some(Partner { id: unhex(id)?, host: host.into(), secret: unhex(secret)?, os: os.into(), name: name.into() })
    };
    text.lines()
        .map(|l| parse(l).ok_or_else(|| format!("{} is damaged: delete it and pair again (`synergy-tools pair`)", path.display())))
        .collect()
}

pub fn save_partners_to(dir: &Path, ps: &[Partner]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let tmp = dir.join("partners.tmp");
    let text: String =
        ps.iter().map(|p| format!("{} {} {} {} {}\n", hex(&p.id), p.host, hex(&p.secret), p.os, p.name)).collect();
    let write = || -> std::io::Result<()> {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        std::io::Write::write_all(&mut opts.open(&tmp)?, text.as_bytes())?;
        std::fs::rename(&tmp, dir.join("partners"))
    };
    write().map_err(|e| format!("cannot save {}: {e}", dir.join("partners").display()))
}

/// Replaces the partner with the same (name, os), else appends.
pub fn add_partner_to(dir: &Path, p: Partner) -> Result<(), String> {
    let mut ps = load_partners_from(dir)?;
    match ps.iter_mut().find(|q| q.name == p.name && q.os == p.os) {
        Some(q) => *q = p,
        None => ps.push(p),
    }
    save_partners_to(dir, &ps)
}

pub fn load_partners() -> Result<Vec<Partner>, String> {
    load_partners_from(&config_dir()?)
}

pub fn save_partners(ps: &[Partner]) -> Result<(), String> {
    save_partners_to(&config_dir()?, ps)
}

pub fn add_partner(p: Partner) -> Result<(), String> {
    add_partner_to(&config_dir()?, p)
}

/// By name (ASCII case-insensitive) or host.
pub fn find<'a>(ps: &'a [Partner], key: &str) -> Result<&'a Partner, String> {
    if ps.is_empty() {
        return Err(NOT_PAIRED.into());
    }
    ps.iter().find(|p| p.name.eq_ignore_ascii_case(key) || p.host == key).ok_or_else(|| {
        let names: Vec<&str> = ps.iter().map(|p| p.name.as_str()).collect();
        format!("no partner {key:?}; partners: {}", names.join(", "))
    })
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

/// "host" or "host:port" to an IPv4 address.
pub fn resolve(target: &str, default_port: u16) -> Result<SocketAddr, String> {
    if target.contains(':') { target.to_socket_addrs() } else { (target, default_port).to_socket_addrs() }
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

    fn partner(name: &str, os: &str, host: &str, b: u8) -> Partner {
        Partner { id: [b; 8], host: host.into(), secret: [b; 32], os: os.into(), name: name.into() }
    }

    #[test]
    fn partner_debug_hides_secret() {
        let p = partner("pc", "linux", "h", 0xab);
        let d = format!("{p:?}");
        assert!(!d.contains(&hex(&p.secret)) && !d.contains("171") && d.contains("<redacted>"), "{d}");
    }

    #[test]
    fn partners_round_trip() {
        let d = tmp();
        let ps = vec![partner("pc-a", "linux", "192.168.1.5", 1), partner("Living Room PC", "windows", "192.168.1.6", 2)];
        save_partners_to(&d, &ps).unwrap();
        assert!(load_partners_from(&d).unwrap() == ps);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn add_replaces_same_name_and_os() {
        let d = tmp();
        add_partner_to(&d, partner("pc", "windows", "10.0.0.1", 1)).unwrap();
        add_partner_to(&d, partner("pc", "windows", "10.0.0.2", 2)).unwrap();
        let ps = load_partners_from(&d).unwrap();
        assert_eq!(ps.len(), 1);
        assert_eq!((ps[0].host.as_str(), ps[0].secret), ("10.0.0.2", [2; 32]));
        add_partner_to(&d, partner("pc", "linux", "10.0.0.2", 3)).unwrap();
        assert_eq!(load_partners_from(&d).unwrap().len(), 2);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn missing_partners_file_is_empty() {
        assert!(load_partners_from(&tmp()).unwrap().is_empty());
    }

    #[test]
    fn damaged_partners_file_rejected() {
        let d = tmp();
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("partners"), "zz host 00 linux pc\n").unwrap();
        assert!(load_partners_from(&d).unwrap_err().contains("pair again"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn partners_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp();
        save_partners_to(&d, &[partner("pc", "linux", "h", 1)]).unwrap();
        let mode = std::fs::metadata(d.join("partners")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn set_host_keeps_secret() {
        let d = tmp();
        save_partners_to(&d, &[partner("pc", "windows", "10.0.0.1", 4)]).unwrap();
        let mut ps = load_partners_from(&d).unwrap();
        let id = find(&ps, "pc").unwrap().id;
        ps.iter_mut().find(|p| p.id == id).unwrap().host = "10.0.0.9".into();
        save_partners_to(&d, &ps).unwrap();
        let p = &load_partners_from(&d).unwrap()[0];
        assert_eq!((p.host.as_str(), p.secret, p.id), ("10.0.0.9", [4; 32], [4; 8]));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn find_by_name_or_host() {
        let ps = vec![partner("pc-a", "linux", "10.0.0.1", 1), partner("pc-b", "windows", "10.0.0.2", 2)];
        assert_eq!(find(&ps, "PC-A").unwrap().name, "pc-a");
        assert_eq!(find(&ps, "10.0.0.2").unwrap().name, "pc-b");
        let e = find(&ps, "nope").unwrap_err();
        assert!(e.contains("pc-a") && e.contains("pc-b"), "{e}");
    }

    #[test]
    fn resolve_default_port() {
        assert_eq!(resolve("127.0.0.1", 24851).unwrap().port(), 24851);
    }
}
