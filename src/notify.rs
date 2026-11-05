//! Desktop notifications. A failure to show one is logged to stderr, never fatal.

use crate::transfer::Received;
use std::path::Path;

const TITLE: &str = "synergy-dragndrop";

pub fn received_text(n: usize, from: &str) -> String {
    format!("{n} file{} received from {from}", if n == 1 { "" } else { "s" })
}

pub fn open_folder(dir: &Path) {
    let opener = if cfg!(windows) { "explorer.exe" } else { "xdg-open" };
    if let Err(e) = std::process::Command::new(opener).arg(dir).spawn() {
        eprintln!("can't open {}: {e}", dir.display());
    }
}

/// Fire-and-forget toast; returns promptly.
pub fn info(text: &str) {
    toast(text, None);
}

pub fn error(text: &str) {
    toast(text, None);
}

/// Toast for received files; clicking it opens the folder they landed in (the Downloads folder).
pub fn received(r: &Received) {
    let dir = r.files.first().and_then(|f| f.parent()).map(Path::to_path_buf);
    toast(&received_text(r.files.len(), &r.from), dir.as_deref());
}

#[cfg(target_os = "linux")]
fn toast(text: &str, open: Option<&Path>) {
    let mut n = notify_rust::Notification::new();
    n.summary(TITLE).body(text);
    if open.is_some() {
        n.action("default", "Open Downloads");
    }
    match n.show() {
        // The click wait runs in its own thread so the caller never blocks; only `listen` is long-lived enough to see a click.
        Ok(h) => {
            if let Some(dir) = open.map(Path::to_path_buf) {
                std::thread::spawn(move || h.wait_for_action(|a| if a == "default" { open_folder(&dir) }));
            }
        }
        Err(e) => eprintln!("notification failed: {e}"),
    }
}

#[cfg(windows)]
fn toast(text: &str, open: Option<&Path>) {
    use tauri_winrt_notification::Toast;
    let mut t = Toast::new(Toast::POWERSHELL_APP_ID).title(TITLE).text1(text);
    if let Some(dir) = open.map(Path::to_path_buf) {
        t = t.on_activated(move |_| {
            open_folder(&dir);
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
