//! Desktop notifications. A failure to show one is logged to stderr, never fatal.

use crate::transfer::Received;
use std::path::Path;

const TITLE: &str = "synergy-tools";

pub fn received_text(n: usize, from: &str) -> String {
    format!("{n} file{} received from {from}", if n == 1 { "" } else { "s" })
}

pub fn open_folder(dir: &Path) {
    let opener = if cfg!(windows) { "explorer.exe" } else { "xdg-open" };
    if let Err(e) = std::process::Command::new(opener).arg(dir).spawn() {
        eprintln!("can't open {}: {e}", dir.display());
    }
}

/// Puts `text` on the clipboard via the OS tool (`xclip` on Linux, `clip` on Windows).
pub fn copy(text: &str) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut cmd = if cfg!(windows) { Command::new("clip") } else { Command::new("xclip") };
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // CREATE_NO_WINDOW: no console flash
    #[cfg(not(windows))]
    cmd.args(["-selection", "clipboard"]);
    let r = cmd.stdin(Stdio::piped()).spawn().and_then(|mut c| {
        c.stdin.take().expect("piped").write_all(text.as_bytes())?;
        c.wait().map(|_| ())
    });
    if let Err(e) = r {
        eprintln!("can't copy to clipboard: {e}");
    }
}

type OnClick = Box<dyn Fn() + Send + Sync + 'static>;

/// Fire-and-forget toast; returns promptly.
pub fn info(text: &str) {
    toast(text, None, None);
}

pub fn error(text: &str) {
    toast(text, None, None);
}

/// Toast showing the pairing command; clicking it (middle-click on dunst) copies the command.
pub fn pair_command(command: &str) {
    let c = command.to_string();
    toast(&format!("On the other PC run: {command}"), Some("Copy command"), Some(Box::new(move || copy(&c))));
}

/// Toast for received files; clicking it opens the folder they landed in (the Downloads folder).
pub fn received(r: &Received) {
    let click = r.files.first().and_then(|f| f.parent()).map(Path::to_path_buf).map(|dir| -> OnClick { Box::new(move || open_folder(&dir)) });
    toast(&received_text(r.files.len(), &r.from), Some("Open Downloads"), click);
}

#[cfg(target_os = "linux")]
fn toast(text: &str, label: Option<&str>, click: Option<OnClick>) {
    let mut n = notify_rust::Notification::new();
    n.summary(TITLE).body(text);
    if let (Some(l), Some(_)) = (label, &click) {
        n.action("default", l);
    }
    match n.show() {
        // The click wait runs in its own thread so the caller never blocks; only `listen` is long-lived enough to see a click.
        Ok(h) => {
            if let Some(f) = click {
                std::thread::spawn(move || h.wait_for_action(|a| if a == "default" { f() }));
            }
        }
        Err(e) => eprintln!("notification failed: {e}"),
    }
}

#[cfg(windows)]
fn toast(text: &str, _label: Option<&str>, click: Option<OnClick>) {
    use tauri_winrt_notification::Toast;
    let mut t = Toast::new(Toast::POWERSHELL_APP_ID).title(TITLE).text1(text);
    if let Some(f) = click {
        t = t.on_activated(move |_| {
            f();
            Ok(())
        });
    }
    if let Err(e) = t.show() {
        eprintln!("notification failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn received_text_plural() {
        assert_eq!(received_text(1, "pc"), "1 file received from pc");
        assert_eq!(received_text(3, "pc"), "3 files received from pc");
    }
}
