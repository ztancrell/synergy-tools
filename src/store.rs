//! Receive-to-folder: safe file names, a hidden temp file, and a no-overwrite finish.
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

const MAX_NAME: usize = 200;

/// Reduce a peer-supplied name to a plain, Windows-legal file name (never a path).
pub fn sanitize_name(raw: &str) -> Option<String> {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let mut name: String = last
        .chars()
        .map(|c| if "<>:\"|?*".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    name.truncate(name.trim_end_matches(['.', ' ']).len());
    if name.is_empty() {
        return None;
    }
    let stem = name.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if reserved {
        name.insert(0, '_');
    }
    if name.len() > MAX_NAME {
        let ext = match name.rfind('.') {
            Some(i) if i > 0 && name.len() - i <= 20 => name[i..].to_string(),
            _ => String::new(),
        };
        let mut cut = MAX_NAME - ext.len();
        while !name.is_char_boundary(cut) {
            cut -= 1;
        }
        name.truncate(cut);
        name.truncate(name.trim_end_matches(['.', ' ']).len());
        name.push_str(&ext);
    }
    Some(name)
}

/// `n == 0` is the name itself; otherwise `stem (n).ext`, splitting at the last dot.
pub fn candidate(name: &str, n: u32) -> String {
    if n == 0 {
        return name.to_string();
    }
    match name.rfind('.') {
        Some(i) if i > 0 => format!("{} ({n}){}", &name[..i], &name[i..]),
        _ => format!("{name} ({n})"),
    }
}

/// A file being received. It only gets its real name in `finish`; dropping it deletes the temp.
pub struct Incoming {
    file: File,
    tmp: PathBuf,
    dir: PathBuf,
    name: String,
    written: u64,
}

pub fn begin(dir: &Path, name: &str) -> Result<Incoming, String> {
    let mut r = [0u8; 4];
    getrandom::fill(&mut r).map_err(|e| format!("random: {e}"))?;
    let tmp = dir.join(format!(".{name}.{}.part", crate::config::hex(&r)));
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o644);
    let file = opts.open(&tmp).map_err(|e| format!("cannot create {}: {e}", tmp.display()))?;
    let inc = Incoming { file, tmp, dir: dir.to_path_buf(), name: name.to_string(), written: 0 };
    // Mark of the Web: the stream follows the file through the hard link in `finish`.
    #[cfg(windows)]
    {
        let mut ads = inc.tmp.clone().into_os_string();
        ads.push(":Zone.Identifier");
        std::fs::write(&ads, "[ZoneTransfer]\r\nZoneId=3\r\n").map_err(|e| format!("cannot mark {name} as downloaded: {e}"))?;
    }
    // Quarantine: Gatekeeper checks the file before it is opened. xattrs are per inode, so
    // the hard link in `finish` keeps it. Best effort: a failure doesn't fail the receive.
    #[cfg(target_os = "macos")]
    {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        match std::process::Command::new("xattr").args(["-w", "com.apple.quarantine", &quarantine_value(secs)]).arg(&inc.tmp).status() {
            Ok(s) if s.success() => {}
            Ok(s) => eprintln!("cannot mark {name} as quarantined: xattr {s}"),
            Err(e) => eprintln!("cannot mark {name} as quarantined: {e}"),
        }
    }
    Ok(inc)
}

#[cfg(any(target_os = "macos", test))]
fn quarantine_value(unix_secs: u64) -> String {
    format!("0081;{unix_secs:x};synergy-tools;")
}

impl Incoming {
    pub fn write(&mut self, b: &[u8]) -> Result<(), String> {
        self.file.write_all(b).map_err(|e| format!("cannot write {}: {e}", self.name))?;
        self.written += b.len() as u64;
        Ok(())
    }

    /// Check the size, then give the file its real name without ever replacing a file.
    pub fn finish(self, expected: u64) -> Result<PathBuf, String> {
        if self.written != expected {
            return Err(format!("{} is incomplete ({} of {expected} bytes)", self.name, self.written));
        }
        self.file.sync_all().map_err(|e| format!("cannot save {}: {e}", self.name))?;
        for n in 0.. {
            let dest = self.dir.join(candidate(&self.name, n));
            match std::fs::hard_link(&self.tmp, &dest) {
                Ok(()) => return Ok(dest), // Drop removes the temp name
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                Err(_) => return self.rename_fallback(n),
            }
        }
        unreachable!()
    }

    // No hard links here (FAT/exFAT): rename if the name is free.
    // ponytail: small check-then-rename race on these filesystems; fine for a two-PC LAN tool.
    fn rename_fallback(&self, from: u32) -> Result<PathBuf, String> {
        for n in from.. {
            let dest = self.dir.join(candidate(&self.name, n));
            if !dest.exists() {
                std::fs::rename(&self.tmp, &dest).map_err(|e| format!("cannot save {}: {e}", self.name))?;
                return Ok(dest);
            }
        }
        unreachable!()
    }
}

impl Drop for Incoming {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.tmp);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn quarantine_value_format() {
        assert_eq!(super::quarantine_value(255), "0081;ff;synergy-tools;");
    }

    use super::*;

    fn tmpdir() -> PathBuf {
        let mut r = [0u8; 8];
        getrandom::fill(&mut r).unwrap();
        let d = std::env::temp_dir().join(format!("sdd-store-{}", crate::config::hex(&r)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn names(d: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    fn receive(d: &Path, name: &str, data: &[u8]) -> PathBuf {
        let mut i = begin(d, name).unwrap();
        i.write(data).unwrap();
        i.finish(data.len() as u64).unwrap()
    }

    #[test] fn sanitize_strips_paths() { assert_eq!(sanitize_name("../../etc/x").as_deref(), Some("x"));
        assert_eq!(sanitize_name(r"C:\Users\a\b.exe").as_deref(), Some("b.exe")); }
    #[test] fn sanitize_windows_illegal() { assert_eq!(sanitize_name("a:b?.txt").as_deref(), Some("a_b_.txt"));
        assert_eq!(sanitize_name("CON").as_deref(), Some("_CON")); assert_eq!(sanitize_name("aux.txt").as_deref(), Some("_aux.txt"));
        assert_eq!(sanitize_name("x. ").as_deref(), Some("x")); }
    #[test] fn sanitize_rejects_empty() { for s in ["", ".", "..", "a/"] { assert_eq!(sanitize_name(s), None) } }
    #[test] fn sanitize_long_name_keeps_ext() { let n = sanitize_name(&("é".repeat(300) + ".zip")).unwrap();
        assert!(n.len() <= 200 && n.ends_with(".zip")); }
    #[test] fn candidate_names() { assert_eq!(candidate("a.zip", 0), "a.zip"); assert_eq!(candidate("a.zip", 2), "a (2).zip");
        assert_eq!(candidate("a.tar.gz", 1), "a.tar (1).gz"); assert_eq!(candidate(".bashrc", 1), ".bashrc (1)");
        assert_eq!(candidate("README", 1), "README (1)"); }

    #[test]
    fn temp_not_visible_until_finished() {
        let d = tmpdir();
        let mut i = begin(&d, "x.zip").unwrap();
        i.write(b"abc").unwrap();
        let n = names(&d);
        assert_eq!(n.len(), 1);
        assert!(n[0].starts_with(".x.zip.") && n[0].ends_with(".part"), "{n:?}");
        drop(i);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn finish_checks_size() {
        let d = tmpdir();
        let mut i = begin(&d, "x.zip").unwrap();
        i.write(b"abc").unwrap();
        assert!(i.finish(4).is_err());
        assert!(names(&d).is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn same_name_twice_in_batch() {
        let d = tmpdir();
        let (mut a, mut b) = (begin(&d, "x.zip").unwrap(), begin(&d, "x.zip").unwrap());
        a.write(b"one").unwrap();
        b.write(b"two").unwrap();
        a.finish(3).unwrap();
        b.finish(3).unwrap();
        assert_eq!(names(&d), ["x (1).zip", "x.zip"]);
        assert_eq!(std::fs::read(d.join("x.zip")).unwrap(), b"one");
        assert_eq!(std::fs::read(d.join("x (1).zip")).unwrap(), b"two");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn finish_never_replaces_existing() {
        let d = tmpdir();
        std::fs::write(d.join("x.zip"), "old").unwrap();
        let p = receive(&d, "x.zip", b"new");
        assert_eq!(p, d.join("x (1).zip"));
        assert_eq!(std::fs::read(d.join("x.zip")).unwrap(), b"old");
        assert_eq!(std::fs::read(&p).unwrap(), b"new");
        assert_eq!(names(&d).len(), 2);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn drop_removes_temp() {
        let d = tmpdir();
        let mut i = begin(&d, "x.zip").unwrap();
        i.write(b"abc").unwrap();
        drop(i);
        assert!(names(&d).is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn received_file_not_executable() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmpdir();
        let p = receive(&d, "x.zip", b"abc");
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o111, 0);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
