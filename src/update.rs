//! `synergy-tools update` and the tray's Update item: download this OS's build from the rolling
//! `latest` GitHub release with the system curl, check it, swap it in for the running exe.

use crate::install::{make_exec, spawn_detached, with_ext};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Crate version plus the git short hash it was built from (build.rs).
pub const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_HASH"), ")");
const URL: &str = "https://github.com/ztancrell/synergy-tools/releases/download/latest";

/// The release asset for an OS/arch (`std::env::consts`), as release.yml names them.
fn asset(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("linux", "x86_64") => Some("synergy-tools-linux-x86_64"),
        ("windows", "x86_64") => Some("synergy-tools-windows-x86_64.exe"),
        ("macos", "x86_64" | "aarch64") => Some("synergy-tools-macos-universal"),
        _ => None,
    }
}

/// Executable magic for `os`; an empty file or an HTML error page doesn't pass.
fn looks_like_exe(os: &str, bytes: &[u8]) -> bool {
    match os {
        "windows" => bytes.starts_with(b"MZ"),
        "linux" => bytes.starts_with(b"\x7fELF"),
        // Universal (fat) or 64-bit Mach-O.
        "macos" => bytes.starts_with(b"\xca\xfe\xba\xbe") || bytes.starts_with(b"\xcf\xfa\xed\xfe"),
        _ => false,
    }
}

/// Where the download goes, next to the exe (Windows keeps `.exe` so it can be run for `--version`).
fn new_path(exe: &Path) -> PathBuf {
    exe.with_extension(if exe.extension().is_some_and(|e| e == "exe") { "new.exe" } else { "new" })
}

/// `…/synergy-tools.app` when `exe` is the binary inside an app bundle.
#[cfg(target_os = "macos")]
fn app_of(exe: &Path) -> Option<&Path> {
    exe.ancestors().nth(3).filter(|a| a.extension().is_some_and(|e| e == "app"))
}

/// The result line for the CLI and the tray.
pub fn done(new: &Option<String>) -> String {
    match new {
        None => format!("Already up to date: synergy-tools {VERSION}"),
        Some(v) => format!("Updated: synergy-tools {VERSION} -> {v}"),
    }
}

/// Download the latest build next to `exe`, check it, swap it in. Ok(Some(new version)) if swapped.
pub fn update(exe: &Path) -> Result<Option<String>, String> {
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    let name = asset(os, arch).ok_or_else(|| format!("no release build for {os}/{arch}"))?;
    let new = new_path(exe);
    // Unique query: some GitHub edges cache the `latest` redirect and point at a deleted asset (404).
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut c = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    c.args(["-fsSL", "--connect-timeout", "20", "-o"]).arg(&new).arg(format!("{URL}/{name}?t={t}")).stdin(Stdio::null());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut c, 0x0800_0000); // CREATE_NO_WINDOW (tray)
    let need = if cfg!(windows) { " (needs Windows 10 1803 or later)" } else { "" };
    let out = c.output().map_err(|e| format!("can't run curl{need}: {e}"))?;
    let r = if out.status.success() {
        swap(os, exe, &new)
    } else {
        Err(format!("download failed: {}", String::from_utf8_lossy(&out.stderr).trim()))
    };
    let _ = fs::remove_file(&new); // already gone after a swap
    r
}

fn swap(os: &str, exe: &Path, new: &Path) -> Result<Option<String>, String> {
    let bytes = fs::read(new).map_err(|e| format!("can't read {}: {e}", new.display()))?;
    if !looks_like_exe(os, &bytes) {
        return Err(format!("the download isn't a {os} program ({} bytes); nothing changed", bytes.len()));
    }
    make_exec(new).map_err(|e| format!("can't make {} executable: {e}", new.display()))?;
    let out = Command::new(new).arg("--version").stdin(Stdio::null()).output().map_err(|e| format!("can't run the download: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let version = stdout.trim().trim_start_matches("synergy-tools ");
    if !out.status.success() || version.is_empty() {
        return Err("the downloaded synergy-tools doesn't run; nothing changed".into());
    }
    if version == VERSION {
        return Ok(None);
    }
    // A running exe can't be overwritten (Windows lock, "text file busy") but can be renamed.
    let old = with_ext(exe, ".old");
    let _ = fs::remove_file(&old);
    fs::rename(exe, &old).map_err(|e| format!("can't move {} aside: {e}", exe.display()))?;
    if let Err(e) = fs::rename(new, exe) {
        let _ = fs::rename(&old, exe);
        return Err(format!("can't put the new version in place: {e}; nothing changed"));
    }
    // Unix deletes it now (the running tray keeps its inode; macOS: not left in the bundle to
    // sign); Windows can't while it runs, so the next update does.
    let _ = fs::remove_file(&old);
    // As install does: re-sign the bundle, whose seal covers the binary just replaced.
    #[cfg(target_os = "macos")]
    if let Some(app) = app_of(exe) {
        let c: Vec<String> = ["codesign", "--force", "--sign", "-"].iter().map(|s| s.to_string()).chain([app.display().to_string()]).collect();
        crate::install::run_cmd(&c);
    }
    Ok(Some(version.to_string()))
}

/// Start the updated tray detached: macOS through LaunchServices as install does, so the audio
/// permission stays with the app (`-n`: this instance is still running).
pub fn restart(exe: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    if let Some(app) = app_of(exe) {
        use std::ffi::OsStr;
        return spawn_detached(&[OsStr::new("/usr/bin/open"), OsStr::new("-n"), app.as_os_str()]);
    }
    spawn_detached(&[exe.as_os_str()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_asset_per_os() {
        assert_eq!(asset("linux", "x86_64"), Some("synergy-tools-linux-x86_64"));
        assert_eq!(asset("windows", "x86_64"), Some("synergy-tools-windows-x86_64.exe"));
        assert_eq!(asset("macos", "aarch64"), Some("synergy-tools-macos-universal"));
        assert_eq!(asset("macos", "x86_64"), Some("synergy-tools-macos-universal"));
        assert_eq!(asset("linux", "aarch64"), None);
        assert_eq!(asset("freebsd", "x86_64"), None);
    }

    #[test]
    fn update_magic_per_os() {
        assert!(looks_like_exe("windows", b"MZ\x90\x00"));
        assert!(looks_like_exe("linux", b"\x7fELF\x02\x01"));
        assert!(looks_like_exe("macos", b"\xca\xfe\xba\xbe\x00")); // universal
        assert!(looks_like_exe("macos", b"\xcf\xfa\xed\xfe\x0c")); // arm64 / x86_64
        for os in ["windows", "linux", "macos"] {
            assert!(!looks_like_exe(os, b""), "{os}");
            assert!(!looks_like_exe(os, b"<!DOCTYPE html>"), "{os}");
        }
        assert!(!looks_like_exe("linux", b"MZ\x90\x00"));
        assert!(!looks_like_exe("windows", b"\x7fELF"));
        assert!(!looks_like_exe("macos", b"\x7fELF"));
    }

    #[test]
    fn update_paths() {
        assert_eq!(new_path(Path::new("C:/p/synergy-tools.exe")), PathBuf::from("C:/p/synergy-tools.new.exe"));
        assert_eq!(new_path(Path::new("/h/.local/bin/synergy-tools")), PathBuf::from("/h/.local/bin/synergy-tools.new"));
        assert_eq!(with_ext(Path::new("C:/p/synergy-tools.exe"), ".old"), PathBuf::from("C:/p/synergy-tools.exe.old"));
        assert_eq!(with_ext(Path::new("/h/.local/bin/synergy-tools"), ".old"), PathBuf::from("/h/.local/bin/synergy-tools.old"));
    }

    #[test]
    fn update_version_has_hash() {
        let hash = VERSION.strip_prefix(concat!(env!("CARGO_PKG_VERSION"), " (")).and_then(|r| r.strip_suffix(')'));
        assert!(hash.is_some_and(|h| !h.is_empty() && !h.contains(' ')), "{VERSION}");
    }
}
