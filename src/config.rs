use std::net::{SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};

/// A paired PC: its id (from the secret), where to reach it, the shared secret, and who it said it is.
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
    let damaged = || format!("{} is damaged: delete it and pair again", path.display());
    text.lines()
        .map(|line| match line.splitn(5, ' ').collect::<Vec<_>>()[..] {
            [id, host, secret, os, name] if !host.is_empty() && !os.is_empty() && !name.is_empty() => Ok(Partner {
                id: unhex(id).ok_or_else(damaged)?,
                host: host.into(),
                secret: unhex(secret).ok_or_else(damaged)?,
                os: os.into(),
                name: name.into(),
            }),
            _ => Err(damaged()),
        })
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

/// Pairing again with the same PC (same name and OS) replaces it; anything else is added.
// ponytail: load-modify-save without a lock, through a shared partners.tmp; a CLI and the tray
// writing at once can lose an update. Per-process tmp name + lock file if it ever matters.
pub fn add_partner_to(dir: &Path, p: Partner) -> Result<(), String> {
    let mut ps = load_partners_from(dir)?;
    match ps.iter_mut().find(|q| q.name == p.name && q.os == p.os) {
        Some(q) => *q = p,
        None => ps.push(p),
    }
    save_partners_to(dir, &ps)
}

/// Windows auto-mute is on unless the `auto-mute-off` marker exists in `dir`.
pub fn auto_mute_on_in(dir: &Path) -> bool {
    !dir.join("auto-mute-off").exists()
}

pub fn set_auto_mute_in(dir: &Path, on: bool) -> Result<(), String> {
    let m = dir.join("auto-mute-off");
    let r = if on {
        std::fs::remove_file(&m).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) })
    } else {
        std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&m, ""))
    };
    r.map_err(|e| format!("can't save the auto-mute setting in {}: {e}", dir.display()))
}

pub fn auto_mute_on() -> bool {
    config_dir().map_or(true, |d| auto_mute_on_in(&d))
}

#[cfg_attr(not(windows), allow(dead_code))] // the toggle is Windows-only
pub fn set_auto_mute(on: bool) -> Result<(), String> {
    set_auto_mute_in(&config_dir()?, on)
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

/// By name or `name/os` (ASCII case-insensitive), or by host. Several matches are an error.
pub fn find<'a>(ps: &'a [Partner], key: &str) -> Result<&'a Partner, String> {
    if ps.is_empty() {
        return Err(NOT_PAIRED.into());
    }
    let named = |p: &Partner| {
        p.name.eq_ignore_ascii_case(key)
            || key.rsplit_once('/').is_some_and(|(n, os)| p.name.eq_ignore_ascii_case(n) && p.os.eq_ignore_ascii_case(os))
    };
    let list = |ps: &[&Partner]| ps.iter().map(|p| format!("{} ({})", p.name, p.os)).collect::<Vec<_>>().join(", ");
    match ps.iter().filter(|p| named(p) || p.host == key).collect::<Vec<_>>()[..] {
        [p] => Ok(p),
        [] => Err(format!("no partner {key:?}; partners: {}", list(&ps.iter().collect::<Vec<_>>()))),
        ref several => Err(format!("{key:?} matches several partners: {}; use <name>/<os>", list(several))),
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
    #[cfg(target_os = "macos")]
    let name = std::process::Command::new("scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    #[cfg(not(any(windows, target_os = "macos")))]
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

    fn partner(name: &str, os: &str, host: &str, secret: u8) -> Partner {
        Partner { id: [secret; 8], host: host.into(), secret: [secret; 32], os: os.into(), name: name.into() }
    }

    #[test]
    fn auto_mute_default_on_and_saved() {
        let d = tmp();
        assert!(auto_mute_on_in(&d), "fresh dir: on");
        set_auto_mute_in(&d, false).unwrap();
        assert!(!auto_mute_on_in(&d));
        set_auto_mute_in(&d, true).unwrap();
        assert!(auto_mute_on_in(&d));
        assert!(!d.join("auto-mute-off").exists(), "marker gone");
        let _ = std::fs::remove_dir_all(&d);
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
        let ps = vec![partner("pc-a", "linux", "192.168.1.5", 1), partner("Living Room PC", "windows", "10.0.0.2:24851", 2), partner("Zach’s MacBook Pro", "macos", "192.168.1.9", 3)];
        save_partners_to(&d, &ps).unwrap();
        assert_eq!(load_partners_from(&d).unwrap(), ps);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_pc_name_from_scutil() {
        assert_ne!(this_pc_name(), "other PC");
    }

    #[test]
    fn add_replaces_same_name_and_os() {
        let d = tmp();
        add_partner_to(&d, partner("pc", "linux", "h1", 1)).unwrap();
        add_partner_to(&d, partner("pc", "linux", "h2", 2)).unwrap();
        let ps = load_partners_from(&d).unwrap();
        assert_eq!(ps, vec![partner("pc", "linux", "h2", 2)]);
        add_partner_to(&d, partner("pc", "windows", "h3", 3)).unwrap();
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
        std::fs::write(d.join("partners"), "0011 host zz linux pc\n").unwrap();
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
        save_partners_to(&d, &[partner("pc-a", "linux", "h1", 1), partner("pc-b", "windows", "h2", 2)]).unwrap();
        let mut ps = load_partners_from(&d).unwrap();
        let i = ps.iter().position(|p| p == find(&ps, "pc-b").unwrap()).unwrap();
        ps[i].host = "h9".into();
        save_partners_to(&d, &ps).unwrap();
        let p = load_partners_from(&d).unwrap().remove(1);
        assert_eq!((p.host.as_str(), p.secret, p.id), ("h9", [2; 32], [2; 8]));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn find_by_name_or_host() {
        let ps = [partner("pc-a", "linux", "10.0.0.1", 1), partner("pc-b", "windows", "10.0.0.2", 2)];
        assert_eq!(find(&ps, "PC-A").unwrap().name, "pc-a");
        assert_eq!(find(&ps, "10.0.0.2").unwrap().name, "pc-b");
        let e = find(&ps, "nope").unwrap_err();
        assert!(e.contains("pc-a") && e.contains("pc-b"), "{e}");
    }

    #[test]
    fn ambiguous_key_needs_os() {
        let ps = [partner("pc", "linux", "10.0.0.1", 1), partner("pc", "windows", "10.0.0.1", 2)];
        let e = find(&ps, "PC").unwrap_err();
        assert!(e.contains("pc (linux)") && e.contains("pc (windows)"), "{e}");
        assert_eq!(find(&ps, "pc/Linux").unwrap().secret, [1; 32]);
        assert_eq!(find(&ps, "PC/windows").unwrap().secret, [2; 32]);
        assert!(find(&ps, "10.0.0.1").unwrap_err().contains("several"));
    }

    #[test]
    fn find_twins_by_name_and_os() {
        let ps = [partner("MBP", "macos", "10.0.0.5", 1), partner("MBP", "windows", "10.0.0.5", 2)];
        assert!(find(&ps, "MBP").unwrap_err().contains("<name>/<os>"));
        assert_eq!(find(&ps, "MBP/windows").unwrap().os, "windows");
        assert!(find(&ps, "10.0.0.5").is_err());
    }

    #[test]
    fn resolve_default_port() {
        assert_eq!(resolve("127.0.0.1", 24851).unwrap().port(), 24851);
        assert_eq!(resolve("127.0.0.1", 24850).unwrap().port(), 24850);
        assert_eq!(resolve("127.0.0.1:9", 24850).unwrap().port(), 9);
    }
}
