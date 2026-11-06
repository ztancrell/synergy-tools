//! Desktop notifications. A failure to show one is logged to stderr, never fatal.

use crate::transfer::Received;
use std::path::Path;

const TITLE: &str = "synergy-tools";

pub fn received_text(n: usize, from: &str) -> String {
    format!("{n} file{} received from {from}", if n == 1 { "" } else { "s" })
}

pub fn open_folder(dir: &Path) {
    let opener = if cfg!(windows) {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(opener).arg(dir).spawn() {
        eprintln!("can't open {}: {e}", dir.display());
    }
}

/// Puts `text` on the clipboard via the OS tool (`xclip` on Linux, `clip` on Windows, `pbcopy` on macOS).
pub fn copy(text: &str) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(if cfg!(windows) {
        "clip"
    } else if cfg!(target_os = "macos") {
        "pbcopy"
    } else {
        "xclip"
    });
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // CREATE_NO_WINDOW: no console flash
    #[cfg(target_os = "linux")]
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

/// osascript arguments; the text and title travel as argv, never inside the script (file names come from the remote partner).
#[cfg(any(target_os = "macos", test))]
fn osascript_args(title: &str, text: &str) -> Vec<String> {
    let mut a: Vec<String> =
        ["-e", "on run argv", "-e", "display notification (item 1 of argv) with title (item 2 of argv)", "-e", "end run"].map(String::from).into();
    a.extend([text.to_string(), title.to_string()]);
    a
}

/// Text only: osascript notifications have no click action.
#[cfg(target_os = "macos")]
fn toast(text: &str, _label: Option<&str>, _click: Option<OnClick>) {
    match std::process::Command::new("osascript").args(osascript_args(TITLE, text)).spawn() {
        Ok(mut c) => {
            std::thread::spawn(move || {
                if let Err(e) = c.wait() {
                    eprintln!("notification failed: {e}");
                }
            });
        }
        Err(e) => eprintln!("notification failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osascript_args_keep_text_out_of_script() {
        let text = r#"a" & (do shell script "touch /tmp/x") & ""#;
        let a = osascript_args("synergy-tools", text);
        assert_eq!(a.len(), 8);
        assert_eq!(a[6], text);
        assert_eq!(a[7], "synergy-tools");
        for i in [0, 2, 4] {
            assert_eq!(a[i], "-e");
        }
        for i in [1, 3, 5] {
            assert!(!a[i].contains("do shell script"));
        }
    }

    #[test]
    fn received_text_plural() {
        assert_eq!(received_text(1, "pc"), "1 file received from pc");
        assert_eq!(received_text(3, "pc"), "3 files received from pc");
    }
}
