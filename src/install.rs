//! `synergy-tools install` / `uninstall`: each OS's work is a pure list of `Step`s, run by `apply`.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub enum Step {
    /// Old-program cleanup: failure ignored.
    Try(Vec<String>),
    /// Failure fails install/uninstall.
    Run(Vec<String>),
    /// Failure -> a note with the command to run as administrator.
    Admin(Vec<String>),
    /// File or folder; missing is fine; failure -> note.
    Delete(PathBuf),
    /// Parents created, replaces.
    Write(PathBuf, String),
    /// Same, mode 0o755 on unix.
    WriteExec(PathBuf, String),
    /// exe -> place: no-op onto itself; the old target is renamed to `<to>.old` first.
    Copy(PathBuf, PathBuf),
    /// Autostart file, marker, line to add; skipped if the file is missing.
    Openbox(PathBuf, &'static str, Option<String>),
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
pub struct Places {
    pub exe: PathBuf,
    pub home: PathBuf,
    pub config: PathBuf,
    pub data: PathBuf,
    /// `%LOCALAPPDATA%` on Windows.
    pub local: PathBuf,
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn places() -> Result<Places, String> {
    let need = |d: Option<PathBuf>, what: &str| d.ok_or_else(|| format!("can't find your {what} folder"));
    Ok(Places {
        exe: std::env::current_exe().map_err(|e| format!("can't find this program's path: {e}"))?,
        home: need(dirs::home_dir(), "home")?,
        config: need(dirs::config_dir(), "config")?,
        data: need(dirs::data_dir(), "data")?,
        local: need(dirs::data_local_dir(), "local data")?,
    })
}

fn show_cmd(c: &[String]) -> String {
    c.iter().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.clone() }).collect::<Vec<_>>().join(" ")
}

fn with_ext(p: &Path, ext: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(ext);
    PathBuf::from(s)
}

fn delete(p: &Path) -> std::io::Result<()> {
    let r = match fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(p),
        Ok(_) => fs::remove_file(p),
        Err(e) => Err(e),
    };
    match r {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn write(p: &Path, text: &str, exec: bool) -> Result<(), String> {
    let err = |e: std::io::Error| format!("can't write {}: {e}", p.display());
    if let Some(d) = p.parent() {
        fs::create_dir_all(d).map_err(err)?;
    }
    fs::write(p, text).map_err(err)?;
    if exec {
        make_exec(p).map_err(err)?;
    }
    Ok(())
}

fn make_exec(_p: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(_p, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    if let (Ok(a), Ok(b)) = (fs::canonicalize(from), fs::canonicalize(to)) {
        if a == b {
            return Ok(()); // install run from the installed copy
        }
    }
    let err = |e: std::io::Error| format!("can't copy {} to {}: {e}", from.display(), to.display());
    if let Some(d) = to.parent() {
        fs::create_dir_all(d).map_err(err)?;
    }
    let (old, new) = (with_ext(to, ".old"), with_ext(to, ".new"));
    let _ = fs::remove_file(&old);
    // A running exe can't be overwritten ("text file busy", Windows lock) but can be renamed.
    let had_old = fs::rename(to, &old).is_ok();
    let r = fs::copy(from, &new).and_then(|_| make_exec(&new)).and_then(|_| fs::rename(&new, to));
    if let Err(e) = r {
        let _ = fs::remove_file(&new);
        if had_old {
            let _ = fs::rename(&old, to);
        }
        return Err(err(e));
    }
    // On Windows this fails while the old tray runs; the next install removes it.
    let _ = fs::remove_file(&old);
    Ok(())
}

/// Ok(notes for the user).
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn apply(steps: &[Step], run: &mut dyn FnMut(&[String]) -> bool) -> Result<Vec<String>, String> {
    let mut notes = vec![];
    for s in steps {
        match s {
            Step::Try(c) => {
                run(c);
            }
            Step::Run(c) => {
                if !run(c) {
                    return Err(format!("{} failed", show_cmd(c)));
                }
            }
            Step::Admin(c) => {
                if !run(c) {
                    // The bare script (last item): pasted into PowerShell, no outer quoting to expand `$`.
                    notes.push(format!("Run this in an administrator PowerShell: {}", c.last().map_or("", |s| s)));
                }
            }
            Step::Delete(p) => {
                if let Err(e) = delete(p) {
                    notes.push(format!("couldn't delete {} ({e}): delete it by hand", p.display()));
                }
            }
            Step::Write(p, t) => write(p, t, false)?,
            Step::WriteExec(p, t) => write(p, t, true)?,
            Step::Copy(a, b) => {
                copy(a, b)?;
                // Don't carry the Mark of the Web of a downloaded exe into the install.
                #[cfg(windows)]
                let _ = fs::remove_file(with_ext(b, ":Zone.Identifier"));
            }
            Step::Openbox(p, marker, add) => {
                let text = match fs::read_to_string(p) {
                    Ok(t) => t,
                    Err(e) if e.kind() == ErrorKind::NotFound => continue,
                    Err(e) => return Err(format!("can't read {}: {e}", p.display())),
                };
                let new = openbox_edit(&text, marker, add.as_deref());
                if new != text {
                    fs::write(p, new).map_err(|e| format!("can't write {}: {e}", p.display()))?;
                }
            }
        }
    }
    Ok(notes)
}

/// Drop every line ending in `# <marker>`, then append `add` (so adding twice keeps one line).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn openbox_edit(text: &str, marker: &str, add: Option<&str>) -> String {
    let tag = format!("# {marker}");
    let mine = |l: &str| l.trim_end().ends_with(&tag);
    if add.is_none() && !text.lines().any(mine) {
        return text.to_string();
    }
    let mut out: String = text.lines().filter(|l| !mine(l)).map(|l| format!("{l}\n")).collect();
    if let Some(a) = add {
        out.push_str(a);
        out.push('\n');
    }
    out
}

/// POSIX shell single quotes.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Desktop Entry Exec argument: quoted, then string-escaped (so `$` is written `\\$`).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn desktop_quote(p: &Path) -> String {
    let mut q = String::from("\"");
    for c in p.to_string_lossy().chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            q.push_str(r"\\");
        }
        q.push(c);
    }
    q.push('"');
    q
}

fn cmd(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn linux(p: &Places, install: bool, openbox: bool) -> Vec<Step> {
    use Step::*;
    let bin = p.home.join(".local/bin/synergy-tools");
    let ob = p.config.join("openbox/autostart");
    let xdg = p.config.join("autostart/synergy-tools.desktop");
    let thunar = p.data.join("Thunar/sendto/synergy-tools.desktop");
    let dolphin = p.data.join("kio/servicemenus/synergy-tools.desktop");
    let nautilus = p.data.join("nautilus/scripts/Send to other PC");
    let old_bin = |n: &str| p.home.join(format!(".local/bin/{n}"));
    let s = |p: &Path| p.display().to_string();
    if !install {
        return vec![
            // Exact cmdline = the tray, never this uninstall.
            Try(cmd(&["pkill", "-x", "-f", &s(&bin)])),
            Delete(bin),
            Delete(xdg),
            Openbox(ob, "synergy-tools", None),
            Delete(thunar),
            Delete(dolphin),
            Delete(nautilus),
        ];
    }
    let q = desktop_quote(&bin);
    let mut v = vec![
        Try(cmd(&["systemctl", "--user", "disable", "--now", "synergy-audio.service"])),
        Try(cmd(&["pkill", "-f", &s(&old_bin("synergy-audio"))])),
        Try(cmd(&["pkill", "-f", &s(&old_bin("synergy-dragndrop"))])),
        Delete(p.config.join("systemd/user/synergy-audio.service")),
        Try(cmd(&["systemctl", "--user", "daemon-reload"])),
        Delete(p.config.join("autostart/synergy-dragndrop.desktop")),
        Openbox(ob.clone(), "synergy-dragndrop", None),
        Delete(p.data.join("Thunar/sendto/synergy-dragndrop.desktop")),
        Delete(p.data.join("kio/servicemenus/synergy-dragndrop.desktop")),
        Delete(p.data.join("nautilus/scripts/Send to other PC")),
        Delete(old_bin("synergy-audio")),
        Delete(old_bin("synergy-dragndrop")),
        Delete(p.config.join("synergy-audio")),
        Delete(p.config.join("synergy-dragndrop")),
        Copy(p.exe.clone(), bin.clone()),
    ];
    if openbox {
        v.push(Delete(xdg));
        v.push(Openbox(ob, "synergy-tools", Some(format!("{} & # synergy-tools", sh_quote(&s(&bin))))));
    } else {
        v.push(Openbox(ob, "synergy-tools", None));
        v.push(Write(
            xdg,
            format!(
                "[Desktop Entry]\nType=Application\nName=synergy-tools\nComment=Audio and files to your other PC\n\
                 Exec={q}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
            ),
        ));
    }
    v.push(Write(
        thunar,
        format!(
            "[Desktop Entry]\nType=Application\nName=Send to other PC\nIcon=document-send\n\
             Exec={q} files send %F\nMimeType=application/octet-stream;\n"
        ),
    ));
    // Plasma 6 only shows executable service menus.
    v.push(WriteExec(
        dolphin,
        format!(
            "[Desktop Entry]\nType=Service\nMimeType=application/octet-stream;\n\
             X-KDE-ServiceTypes=KonqPopupMenu/Plugin\nActions=send\n\n[Desktop Action send]\n\
             Name=Send to other PC\nIcon=document-send\nExec={q} files send %F\n"
        ),
    ));
    v.push(WriteExec(nautilus, format!("#!/bin/sh\nexec {} files send \"$@\"\n", sh_quote(&s(&bin)))));
    v
}

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const FIREWALL_RULE: &str = "synergy-tools (TCP 24851)";

/// PowerShell single-quoted string.
#[cfg_attr(not(windows), allow(dead_code))]
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[cfg_attr(not(windows), allow(dead_code))]
fn ps(script: String) -> Vec<String> {
    cmd(&["powershell", "-NoProfile", "-NonInteractive", "-Command", &script])
}

/// Remove the old dragndrop rule, add ours unless present. Never contains `"` (it is printed as a command).
#[cfg_attr(not(windows), allow(dead_code))]
fn firewall_ps(exe: &Path) -> String {
    let rule = |n: &str| format!("Get-NetFirewallRule -DisplayName '{n} (TCP 24851)' -ErrorAction SilentlyContinue");
    format!(
        "$ErrorActionPreference='Stop'; {} | Remove-NetFirewallRule; if (-not ({})) {{ New-NetFirewallRule \
         -DisplayName '{FIREWALL_RULE}' -Direction Inbound -Protocol TCP -LocalPort {} -Program {} \
         -Profile Private,Public -Action Allow | Out-Null }}",
        rule("synergy-dragndrop"),
        rule("synergy-tools"),
        crate::PORT,
        ps_quote(&exe.display().to_string())
    )
}

#[cfg_attr(not(windows), allow(dead_code))]
fn sendto_ps(lnk: &Path, exe: &Path) -> String {
    format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut({}); $s.TargetPath={}; $s.Arguments='files send'; $s.Save()",
        ps_quote(&lnk.display().to_string()),
        ps_quote(&exe.display().to_string())
    )
}

#[cfg_attr(not(windows), allow(dead_code))]
fn windows_dir(p: &Places) -> PathBuf {
    p.local.join("Programs").join("synergy-tools")
}

#[cfg_attr(not(windows), allow(dead_code))]
fn windows_lnk(p: &Places) -> PathBuf {
    p.config.join("Microsoft").join("Windows").join("SendTo").join("Send to other PC.lnk")
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn windows(p: &Places, install: bool) -> Vec<Step> {
    use Step::*;
    let (dir, lnk) = (windows_dir(p), windows_lnk(p));
    let exe_t = dir.join("synergy-tools.exe");
    let s = |p: &Path| p.display().to_string();
    if !install {
        return vec![
            Try(cmd(&[
                "taskkill",
                "/F",
                "/FI",
                "IMAGENAME eq synergy-tools.exe",
                "/FI",
                &format!("PID ne {}", std::process::id()),
            ])),
            Try(cmd(&["reg", "delete", RUN_KEY, "/v", "synergy-tools", "/f"])),
            Delete(lnk),
            Admin(ps(format!(
                "$ErrorActionPreference='Stop'; Get-NetFirewallRule -DisplayName '{FIREWALL_RULE}' \
                 -ErrorAction SilentlyContinue | Remove-NetFirewallRule"
            ))),
            Delete(dir),
        ];
    }
    let old = |n: &str| p.local.join(n);
    let task = |op: &[&str], n: &str| {
        let mut c = vec!["schtasks"];
        c.extend_from_slice(op);
        c.extend_from_slice(&["/TN", n]);
        Try(cmd(&c))
    };
    vec![
        Try(cmd(&[&s(&old("synergy-audio").join("synergy-audio.exe")), "unmute"])),
        task(&["/End"], "synergy-audio"),
        Try(cmd(&["schtasks", "/Delete", "/TN", "synergy-audio", "/F"])),
        task(&["/End"], "synergy-dragndrop"),
        Try(cmd(&["schtasks", "/Delete", "/TN", "synergy-dragndrop", "/F"])),
        Try(cmd(&["taskkill", "/F", "/IM", "synergy-audio.exe"])),
        Try(cmd(&["taskkill", "/F", "/IM", "synergy-dragndrop.exe"])),
        Delete(old("synergy-audio")),
        Delete(old("synergy-dragndrop")),
        Delete(p.config.join("synergy-audio")),
        Delete(p.config.join("synergy-dragndrop")),
        Delete(lnk.clone()),
        Admin(ps(firewall_ps(&exe_t))),
        Copy(p.exe.clone(), exe_t.clone()),
        Run(cmd(&["reg", "add", RUN_KEY, "/v", "synergy-tools", "/t", "REG_SZ", "/d", &format!("\"{}\"", s(&exe_t)), "/f"])),
        Run(ps(sendto_ps(&lnk, &exe_t))),
    ]
}

#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
pub fn installed_exe(p: &Places) -> PathBuf {
    #[cfg(windows)]
    return windows_dir(p).join("synergy-tools.exe");
    #[cfg(not(windows))]
    p.home.join(".local/bin/synergy-tools")
}

#[cfg(any(target_os = "linux", windows))]
fn run_cmd(c: &[String]) -> bool {
    use std::process::{Command, Stdio};
    let mut k = Command::new(&c[0]);
    k.args(&c[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut k, 0x0800_0000); // CREATE_NO_WINDOW
    k.status().is_ok_and(|s| s.success())
}

/// Start the installed tray unless one already holds TCP `PORT`.
#[cfg(any(target_os = "linux", windows))]
fn start_tray(bin: &Path) {
    use std::process::{Command, Stdio};
    if std::net::TcpListener::bind(("0.0.0.0", crate::PORT)).is_err() {
        println!(
            "a synergy-tools tray is already running: Quit it from its menu and start {} to use this version",
            bin.display()
        );
        return;
    }
    let mut k = Command::new(bin);
    k.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // Own process group: closing the terminal doesn't take the tray with it.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut k, 0);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut k, 0x0000_0208); // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
    match k.spawn() {
        Ok(_) => println!("started {}", bin.display()),
        Err(e) => println!("couldn't start {}: {e}", bin.display()),
    }
}

#[cfg(target_os = "linux")]
pub fn install() -> Result<(), String> {
    const HELPERS: [&str; 4] = [
        "/usr/local/libexec/openbox-xdg-autostart",
        "/usr/libexec/openbox-xdg-autostart",
        "/usr/lib/openbox/openbox-xdg-autostart",
        "/usr/bin/openbox-xdg-autostart",
    ];
    let p = places()?;
    let ob = p.config.join("openbox/autostart");
    let openbox = ob.exists() && !HELPERS.iter().any(|h| Path::new(h).exists());
    let notes = apply(&linux(&p, true, openbox), &mut run_cmd)?;
    let bin = installed_exe(&p);
    let autostart = if openbox { ob } else { p.config.join("autostart/synergy-tools.desktop") };
    println!("installed {}", bin.display());
    println!("autostart at login: {}", autostart.display());
    println!("\"Send to other PC\" added to the Thunar, Dolphin and Nautilus right-click menus");
    for n in notes {
        println!("{n}");
    }
    start_tray(&bin);
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn uninstall() -> Result<(), String> {
    let p = places()?;
    let notes = apply(&linux(&p, false, false), &mut run_cmd)?;
    println!("uninstalled {} (pairings kept in {})", installed_exe(&p).display(), p.config.join("synergy-tools").display());
    for n in notes {
        println!("{n}");
    }
    Ok(())
}

#[cfg(windows)]
pub fn install() -> Result<(), String> {
    let p = places()?;
    let notes = apply(&windows(&p, true), &mut run_cmd)?;
    let exe = installed_exe(&p);
    println!("installed {}", exe.display());
    println!("autostart at login: {RUN_KEY}");
    println!("\"Send to other PC\" added to the Explorer Send to menu");
    for n in notes {
        println!("{n}");
    }
    start_tray(&exe);
    Ok(())
}

#[cfg(windows)]
pub fn uninstall() -> Result<(), String> {
    let p = places()?;
    let notes = apply(&windows(&p, false), &mut run_cmd)?;
    println!("uninstalled {} (pairings kept in {})", installed_exe(&p).display(), p.config.join("synergy-tools").display());
    for n in notes {
        println!("{n}");
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn install() -> Result<(), String> {
    Err("install is not supported on this OS yet".into())
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn uninstall() -> Result<(), String> {
    Err("uninstall is not supported on this OS yet".into())
}

#[cfg(test)]
mod tests {
    use super::Step::*;
    use super::*;

    fn tmp() -> PathBuf {
        let mut r = [0u8; 8];
        getrandom::fill(&mut r).unwrap();
        let d = std::env::temp_dir().join(format!("sdd-test-{}", r.iter().map(|b| format!("{b:02x}")).collect::<String>()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn places_in(t: &Path) -> Places {
        let home = t.join("home");
        Places { exe: t.join("exe"), config: home.join(".config"), data: home.join(".local/share"), local: home.join("Local"), home }
    }

    fn ok(_: &[String]) -> bool {
        true
    }

    fn s(p: &Path) -> String {
        p.display().to_string()
    }

    /// Relative path + bytes of every file under `d`.
    fn snapshot(d: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        fn walk(root: &Path, d: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
            for e in fs::read_dir(d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(root, &p, out);
                } else {
                    out.push((p.strip_prefix(root).unwrap().to_path_buf(), fs::read(&p).unwrap()));
                }
            }
        }
        let mut out = vec![];
        walk(d, d, &mut out);
        out.sort();
        out
    }

    #[test]
    fn openbox_edit_adds_once_and_removes() {
        let l = "'/home/u/.local/bin/synergy-tools' & # synergy-tools";
        let added = openbox_edit("tint2 &\n", "synergy-tools", Some(l));
        assert_eq!(added, format!("tint2 &\n{l}\n"));
        assert_eq!(openbox_edit(&added, "synergy-tools", Some(l)), added);
        assert_eq!(openbox_edit(&added, "synergy-tools", None), "tint2 &\n");
        assert_eq!(openbox_edit("tint2 &", "synergy-tools", Some(l)), format!("tint2 &\n{l}\n"));
        assert_eq!(
            openbox_edit(
                "/home/u/.local/bin/synergy-dragndrop & # synergy-dragndrop\ntint2 &\n",
                "synergy-dragndrop",
                None
            ),
            "tint2 &\n"
        );
    }

    #[test]
    fn desktop_quote_escapes() {
        assert_eq!(desktop_quote(Path::new("/home/a b/x$y")), r#""/home/a b/x\\$y""#);
    }

    #[test]
    fn sh_quote_handles_apostrophe() {
        assert_eq!(sh_quote("/home/o'b/x"), r"'/home/o'\''b/x'");
    }

    #[test]
    fn linux_cleanup_comes_first_in_order() {
        let p = places_in(Path::new("/h"));
        let (c, d, h) = (&p.config, &p.data, &p.home);
        let v = |a: &[&str]| cmd(a);
        let want = vec![
            Try(v(&["systemctl", "--user", "disable", "--now", "synergy-audio.service"])),
            Try(v(&["pkill", "-f", &s(&h.join(".local/bin/synergy-audio"))])),
            Try(v(&["pkill", "-f", &s(&h.join(".local/bin/synergy-dragndrop"))])),
            Delete(c.join("systemd/user/synergy-audio.service")),
            Try(v(&["systemctl", "--user", "daemon-reload"])),
            Delete(c.join("autostart/synergy-dragndrop.desktop")),
            Openbox(c.join("openbox/autostart"), "synergy-dragndrop", None),
            Delete(d.join("Thunar/sendto/synergy-dragndrop.desktop")),
            Delete(d.join("kio/servicemenus/synergy-dragndrop.desktop")),
            Delete(d.join("nautilus/scripts/Send to other PC")),
            Delete(h.join(".local/bin/synergy-audio")),
            Delete(h.join(".local/bin/synergy-dragndrop")),
            Delete(c.join("synergy-audio")),
            Delete(c.join("synergy-dragndrop")),
        ];
        assert_eq!(&linux(&p, true, false)[..14], &want[..]);
    }

    #[test]
    fn linux_autostart_xdg_or_openbox() {
        let p = places_in(Path::new("/h"));
        let bin = s(&p.home.join(".local/bin/synergy-tools"));
        let xdg = p.config.join("autostart/synergy-tools.desktop");
        let x = linux(&p, true, false);
        assert!(x.iter().any(|st| matches!(st, Write(f, t) if *f == xdg && t.contains(&format!("Exec={}\n", desktop_quote(&p.home.join(".local/bin/synergy-tools")))))));
        assert!(!x.contains(&Delete(xdg.clone())));
        let o = linux(&p, true, true);
        assert!(o.contains(&Delete(xdg.clone())));
        assert!(o.contains(&Openbox(p.config.join("openbox/autostart"), "synergy-tools", Some(format!("'{bin}' & # synergy-tools")))));
        assert!(!o.iter().any(|st| matches!(st, Write(f, _) if *f == xdg)));
    }

    #[test]
    fn uninstall_reverses_install() {
        let p = places_in(Path::new("/h"));
        let xdg = p.config.join("autostart/synergy-tools.desktop");
        for b in [false, true] {
            let targets = |st: &Step| match st {
                Write(f, _) | WriteExec(f, _) | Copy(_, f) => Some(f.clone()),
                _ => None,
            };
            let mut added: Vec<PathBuf> = linux(&p, true, b).iter().filter_map(targets).collect();
            let mut deleted: Vec<PathBuf> =
                linux(&p, false, b).into_iter().filter_map(|st| if let Delete(f) = st { Some(f) } else { None }).collect();
            if b {
                deleted.retain(|f| *f != xdg);
            }
            added.sort();
            deleted.sort();
            assert_eq!(added, deleted, "openbox={b}");
        }
    }

    fn wp() -> (Places, PathBuf, PathBuf) {
        let p = places_in(Path::new("/h"));
        let dir = p.local.join("Programs").join("synergy-tools");
        (p, dir.join("synergy-tools.exe"), dir)
    }

    #[test]
    fn windows_cleanup_in_spec_order() {
        let (p, exe_t, _) = wp();
        let (l, c) = (&p.local, &p.config);
        let v = |a: &[&str]| cmd(a);
        let want = vec![
            Try(v(&[&s(&l.join("synergy-audio").join("synergy-audio.exe")), "unmute"])),
            Try(v(&["schtasks", "/End", "/TN", "synergy-audio"])),
            Try(v(&["schtasks", "/Delete", "/TN", "synergy-audio", "/F"])),
            Try(v(&["schtasks", "/End", "/TN", "synergy-dragndrop"])),
            Try(v(&["schtasks", "/Delete", "/TN", "synergy-dragndrop", "/F"])),
            Try(v(&["taskkill", "/F", "/IM", "synergy-audio.exe"])),
            Try(v(&["taskkill", "/F", "/IM", "synergy-dragndrop.exe"])),
            Delete(l.join("synergy-audio")),
            Delete(l.join("synergy-dragndrop")),
            Delete(c.join("synergy-audio")),
            Delete(c.join("synergy-dragndrop")),
            Delete(windows_lnk(&p)),
            Admin(ps(firewall_ps(&exe_t))),
        ];
        assert_eq!(&windows(&p, true)[..13], &want[..]);
        let lnk = s(&windows_lnk(&p));
        assert!(lnk.ends_with("Send to other PC.lnk") && lnk.contains("SendTo"));
    }

    #[test]
    fn windows_adds() {
        let (p, exe_t, _) = wp();
        let v = windows(&p, true);
        assert_eq!(v.len(), 16);
        assert_eq!(v[13], Copy(p.exe.clone(), exe_t.clone()));
        let Run(reg) = &v[14] else { panic!("{:?}", v[14]) };
        assert_eq!(&reg[..7], &cmd(&["reg", "add", RUN_KEY, "/v", "synergy-tools", "/t", "REG_SZ"])[..]);
        assert_eq!(reg[7], "/d");
        assert_eq!(reg[8], format!("\"{}\"", s(&exe_t)));
        assert_eq!(reg[9], "/f");
        let Run(c) = &v[15] else { panic!("{:?}", v[15]) };
        assert_eq!(&c[..4], &cmd(&["powershell", "-NoProfile", "-NonInteractive", "-Command"])[..]);
        assert!(c[4].contains("Send to other PC.lnk"));
        assert!(c[4].contains("$s.Arguments='files send'"));
        assert!(c[4].contains(&format!("TargetPath={}", ps_quote(&s(&exe_t)))));
    }

    #[test]
    fn ps_quote_doubles_single_quotes() {
        assert_eq!(ps_quote(r"C:\Users\Zach O'Brien\x.exe"), r"'C:\Users\Zach O''Brien\x.exe'");
    }

    #[test]
    fn firewall_ps_has_no_double_quotes() {
        let f = firewall_ps(Path::new(r"C:\Users\Zach O'Brien\x.exe"));
        assert!(!f.contains('"'), "{f}");
        for want in [
            "-LocalPort 24851",
            "-Profile Private,Public",
            "'synergy-dragndrop (TCP 24851)'",
            "'synergy-tools (TCP 24851)'",
            r"-Program 'C:\Users\Zach O''Brien\x.exe'",
            "$ErrorActionPreference='Stop'; ",
        ] {
            assert!(f.contains(want), "{want} in {f}");
        }
        assert!(f.find("Remove-NetFirewallRule") < f.find("New-NetFirewallRule"));
    }

    #[test]
    fn windows_uninstall_reverses_install() {
        let (p, exe_t, dir) = wp();
        let (i, u) = (windows(&p, true), windows(&p, false));
        assert!(exe_t.starts_with(&dir));
        assert!(i.iter().any(|st| matches!(st, Copy(_, to) if *to == exe_t)));
        assert!(u.contains(&Delete(dir)));
        assert!(i.contains(&Delete(windows_lnk(&p))) && u.contains(&Delete(windows_lnk(&p))));
        let add = |st: &Step| matches!(st, Run(c) if c[..2] == ["reg", "add"] && c[2..5] == [RUN_KEY, "/v", "synergy-tools"]);
        let del = |st: &Step| matches!(st, Try(c) if c[..2] == ["reg", "delete"] && c[2..5] == [RUN_KEY, "/v", "synergy-tools"]);
        assert!(i.iter().any(add) && u.iter().any(del));
        assert!(matches!(&u[0], Try(c) if c[0] == "taskkill" && c.contains(&format!("PID ne {}", std::process::id()))));
        assert!(matches!(u.iter().find(|st| matches!(st, Admin(_))), Some(Admin(c)) if c[4].contains("Remove-NetFirewallRule") && !c[4].contains("New-")));
    }

    #[test]
    fn apply_twice_is_idempotent() {
        let t = tmp();
        let p = places_in(&t);
        fs::write(&p.exe, b"abc").unwrap();
        let old_bin = p.home.join(".local/bin/synergy-dragndrop");
        let old_xdg = p.config.join("autostart/synergy-dragndrop.desktop");
        let ob = p.config.join("openbox/autostart");
        for (f, text) in [(&old_bin, "x"), (&old_xdg, "x"), (&ob, "tint2 &\n/x/synergy-dragndrop & # synergy-dragndrop\n")] {
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            fs::write(f, text).unwrap();
        }
        let mut cmds = vec![];
        let mut stub = |c: &[String]| {
            cmds.push(c.to_vec());
            true
        };
        apply(&linux(&p, true, true), &mut stub).unwrap();
        let first = snapshot(&t);
        apply(&linux(&p, true, true), &mut stub).unwrap();
        assert_eq!(snapshot(&t), first);
        assert!(!old_bin.exists() && !old_xdg.exists());
        let text = fs::read_to_string(&ob).unwrap();
        assert_eq!(text.lines().filter(|l| l.ends_with("# synergy-tools")).count(), 1);
        assert!(!text.contains("synergy-dragndrop"));
        assert_eq!(fs::read(p.home.join(".local/bin/synergy-tools")).unwrap(), b"abc");

        apply(&linux(&p, false, true), &mut stub).unwrap();
        let left: Vec<PathBuf> = snapshot(&t).into_iter().map(|(f, _)| f).collect();
        let mut want = vec![PathBuf::from("exe"), ob.strip_prefix(&t).unwrap().to_path_buf()];
        want.sort();
        assert_eq!(left, want);
        assert_eq!(fs::read_to_string(&ob).unwrap(), "tint2 &\n");
        assert!(cmds.iter().any(|c| c[0] == "pkill" && c.contains(&"-x".to_string())));
        fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn copy_onto_itself_is_noop() {
        let t = tmp();
        let a = t.join("a");
        fs::write(&a, b"abc").unwrap();
        assert_eq!(apply(&[Copy(a.clone(), a.clone())], &mut ok), Ok(vec![]));
        assert_eq!(fs::read(&a).unwrap(), b"abc");
        fs::remove_dir_all(&t).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn copy_replaces_existing_target() {
        use std::os::unix::fs::PermissionsExt;
        let t = tmp();
        let (a, b) = (t.join("a"), t.join("dir/b"));
        fs::write(&a, b"new").unwrap();
        fs::create_dir_all(b.parent().unwrap()).unwrap();
        fs::write(&b, b"old bytes").unwrap();
        apply(&[Copy(a, b.clone())], &mut ok).unwrap();
        assert_eq!(fs::read(&b).unwrap(), b"new");
        assert_eq!(fs::metadata(&b).unwrap().permissions().mode() & 0o777, 0o755);
        assert!(!with_ext(&b, ".new").exists() && !with_ext(&b, ".old").exists());
        fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn admin_failure_is_a_note_not_an_error() {
        let notes = apply(&[Admin(vec!["x".into(), "y z".into()])], &mut |_| false).unwrap();
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("administrator PowerShell") && notes[0].ends_with(": y z"), "{notes:?}");
        assert!(apply(&[Run(vec!["x".into()])], &mut |_| false).is_err());
        assert_eq!(apply(&[Try(vec!["x".into()])], &mut |_| false), Ok(vec![]));
    }

    #[cfg(unix)]
    #[test]
    fn failed_delete_is_a_note() {
        use std::os::unix::fs::PermissionsExt;
        let t = tmp();
        let sub = t.join("dir/sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("file"), b"x").unwrap();
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o500)).unwrap();
        let r = apply(&[Delete(sub.join("file"))], &mut ok);
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o755)).unwrap();
        let notes = r.unwrap();
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains(&s(&sub.join("file"))) && notes[0].contains("delete it by hand"), "{notes:?}");
        fs::remove_dir_all(&t).unwrap();
    }
}
