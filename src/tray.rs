//! Drop window: a small always-on-top window; files dropped on it are sent to the partner.
//! State is shown in the window title (no drawing crate).

use crate::{config, notify, pair, transfer};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
#[cfg(target_os = "linux")]
use tao::platform::unix::WindowExtUnix as _;
#[cfg(windows)]
use tao::platform::windows::WindowExtWindows as _;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// Requested size; the WM/GTK may enlarge it (200x200 on Openbox), so placement uses the real size.
const SIZE: f64 = 160.0;
/// Gap kept free at the bottom-right (taskbar / panel).
const MARGIN: f64 = 48.0;
const RESET_AFTER: Duration = Duration::from_secs(3);

/// How long the tray stays red after an error.
const ERROR_FOR: Duration = Duration::from_secs(5);

/// Messages from worker threads and the tray menu to the event loop.
pub enum UserEvent {
    /// The user minimized the drop window (Linux: GTK iconified).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Minimized,
    SendDone(usize, Result<String, String>),
    Listener(transfer::Event),
    /// The listener thread died: this app can't receive any more.
    Fatal(String),
    Menu(MenuId),
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    /// Linux: a drag is over the window (tao has no Linux drop events, so GTK reports them).
    Hover(bool),
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    /// Linux: one finished drop, the whole batch.
    Dropped(Vec<PathBuf>),
}

/// `text/uri-list` body -> local paths. Only `file://` URIs (host empty or `localhost`) count.
#[cfg(target_os = "linux")]
fn parse_uri_list(s: &str) -> Vec<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    s.lines()
        .filter_map(|l| {
            let rest = l.strip_prefix("file://")?;
            let path = rest.strip_prefix("localhost").unwrap_or(rest);
            if !path.starts_with('/') {
                return None;
            }
            let b = path.as_bytes();
            let (mut out, mut i) = (Vec::new(), 0);
            while i < b.len() {
                let hex = |c: u8| (c as char).to_digit(16);
                match (b[i], b.get(i + 1).and_then(|c| hex(*c)), b.get(i + 2).and_then(|c| hex(*c))) {
                    (b'%', Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 3;
                    }
                    (c, ..) => {
                        out.push(c);
                        i += 1;
                    }
                }
            }
            Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
        })
        .collect()
}

/// Minimize reaches the event loop as a user event.
#[cfg(target_os = "linux")]
fn gtk_minimized(window: &tao::window::Window, proxy: tao::event_loop::EventLoopProxy<UserEvent>) {
    use gtk::prelude::*;
    window.gtk_window().connect_window_state_event(move |_, e| {
        if e.new_window_state().contains(gtk::gdk::WindowState::ICONIFIED) {
            let _ = proxy.send_event(UserEvent::Minimized);
        }
        gtk::glib::Propagation::Proceed
    });
}

/// Wire GTK drag-and-drop onto the window; results reach the event loop as user events.
#[cfg(target_os = "linux")]
fn gtk_drop(window: &tao::window::Window, proxy: tao::event_loop::EventLoopProxy<UserEvent>) {
    use gtk::prelude::*;
    use tao::platform::unix::WindowExtUnix;
    let w = window.gtk_window();
    w.drag_dest_set(
        gtk::DestDefaults::ALL,
        &[gtk::TargetEntry::new("text/uri-list", gtk::TargetFlags::OTHER_APP, 0)],
        gtk::gdk::DragAction::COPY,
    );
    let p = proxy.clone();
    w.connect_drag_motion(move |_, _, _, _, _| {
        let _ = p.send_event(UserEvent::Hover(true));
        true
    });
    let p = proxy.clone();
    w.connect_drag_leave(move |_, _, _| {
        let _ = p.send_event(UserEvent::Hover(false));
    });
    w.connect_drag_data_received(move |_, _, _, _, data, _, _| {
        let files = parse_uri_list(&String::from_utf8_lossy(&data.data()));
        let _ = proxy.send_event(UserEvent::Dropped(files));
    });
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TrayState {
    Unpaired,
    Idle,
    Busy,
    Error,
}

/// 32x32 filled rounded square: grey / green / blue / red.
fn icon_rgba(state: TrayState) -> Vec<u8> {
    let [r, g, b] = match state {
        TrayState::Unpaired => [128, 128, 128],
        TrayState::Idle => [46, 160, 67],
        TrayState::Busy => [47, 111, 237],
        TrayState::Error => [215, 58, 73],
    };
    const N: i32 = 32;
    const R: i32 = 8; // corner radius
    let mut px = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            // Distance of the pixel centre past the corner circle's centre, per axis.
            let (dx, dy) = ((R - x).max(x - (N - 1 - R)).max(0), (R - y).max(y - (N - 1 - R)).max(0));
            let a = if dx * dx + dy * dy <= R * R { 255 } else { 0 };
            px.extend([r, g, b, a]);
        }
    }
    px
}

#[derive(Debug, PartialEq)]
enum DropState {
    Idle,
    Hover,
    Sending(usize),
    Done(bool),
}

fn title_for(state: &DropState) -> String {
    match state {
        DropState::Idle => "Drop files to send".into(),
        DropState::Hover => "Release to send".into(),
        DropState::Sending(1) => "Sending 1 file…".into(),
        DropState::Sending(n) => format!("Sending {n} files…"),
        DropState::Done(true) => "Sent ✓".into(),
        DropState::Done(false) => "Not sent".into(),
    }
}

/// What the window is doing; sends may overlap, so this counts them.
#[derive(Default)]
struct Tracker {
    in_flight: usize,
    failed: bool,
    hover: bool,
    result: Option<bool>,
}

impl Tracker {
    fn start(&mut self, files: usize) {
        self.in_flight += files;
        self.result = None;
    }
    /// One send of `files` files finished; the result shows once nothing is in flight.
    fn finish(&mut self, files: usize, ok: bool) {
        self.in_flight = self.in_flight.saturating_sub(files);
        self.failed |= !ok;
        if self.in_flight == 0 {
            self.result = Some(!self.failed);
            self.failed = false;
        }
    }
    fn state(&self) -> DropState {
        if self.in_flight > 0 {
            DropState::Sending(self.in_flight)
        } else if self.hover {
            DropState::Hover
        } else {
            self.result.map_or(DropState::Idle, DropState::Done)
        }
    }
}

/// Bottom-right of the primary monitor, by the window's real outer size: the WM/GTK may have
/// enlarged it, and it is only known once the window is mapped.
fn place<T>(window: &tao::window::Window, event_loop: &tao::event_loop::EventLoopWindowTarget<T>) {
    if let Some(m) = event_loop.primary_monitor() {
        let (p, sz, o) = (m.position(), m.size(), window.outer_size());
        let margin = MARGIN * m.scale_factor();
        let x = p.x as f64 + sz.width as f64 - o.width as f64 - margin;
        let y = p.y as f64 + sz.height as f64 - o.height as f64 - margin;
        window.set_outer_position(PhysicalPosition::new(x, y));
    }
}

/// Precedence: error over unpaired over busy over idle.
fn tray_state(paired: bool, error_active: bool, busy: bool) -> TrayState {
    if error_active {
        TrayState::Error
    } else if !paired {
        TrayState::Unpaired
    } else if busy {
        TrayState::Busy
    } else {
        TrayState::Idle
    }
}

fn status_text(paired: Option<&str>) -> String {
    match paired {
        Some(h) => format!("Paired with {h}"),
        None => "Not paired".into(),
    }
}

/// How often the partners file is re-read, so an `unpair` from a terminal shows up.
const PEER_CHECK: Duration = Duration::from_secs(10);

fn tray_icon_for(s: TrayState) -> Icon {
    Icon::from_rgba(icon_rgba(s), 32, 32).expect("32x32 RGBA")
}

fn tooltip(paired: Option<&str>) -> String {
    match paired {
        Some(h) => format!("synergy-tools: paired with {h}"),
        None => "synergy-tools: not paired".into(),
    }
}

/// A tray app started at login has no visible stderr: failures also go to a notification.
pub fn run() -> Result<(), String> {
    start().inspect_err(|e| notify::error(e))
}

fn start() -> Result<(), String> {
    // Single instance: the port is the lock.
    let dir = dirs::download_dir().ok_or_else(|| "can't find your Downloads folder".to_string())?;
    let listener = transfer::Listener::bind(crate::PORT, dir)?;
    let pairing = listener.pairing.clone();

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    {
        let proxy = proxy.clone();
        std::thread::spawn(move || {
            let p2 = proxy.clone();
            // `run` never returns; a panic here would otherwise leave a healthy-looking tray.
            let died = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                listener.run(move |e| {
                    let _ = p2.send_event(UserEvent::Listener(e));
                })
            }));
            let Err(p) = died else { return };
            let msg = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned());
            let _ = proxy.send_event(UserEvent::Fatal(format!(
                "the receiver stopped: {}",
                msg.unwrap_or_else(|| "unknown error".into())
            )));
        });
    }
    {
        let proxy = proxy.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let _ = proxy.send_event(UserEvent::Menu(e.id));
        }));
    }
    let window = WindowBuilder::new()
        .with_title(title_for(&DropState::Idle))
        .with_inner_size(LogicalSize::new(SIZE, SIZE))
        .with_resizable(false)
        .with_always_on_top(true)
        .build(&event_loop)
        .map_err(|e| format!("can't open the drop window: {e}"))?;
    #[cfg(target_os = "linux")]
    {
        gtk_drop(&window, proxy.clone());
        gtk_minimized(&window, proxy.clone());
    }
    let (status, toggle, pair_item, quit) = (
        MenuItem::new(status_text(None), false, None),
        MenuItem::new("Hide drop window", true, None),
        MenuItem::new("Pair…", true, None),
        MenuItem::new("Quit", true, None),
    );
    let menu = Menu::new();
    menu.append_items(&[&status, &toggle, &pair_item, &quit]).map_err(|e| format!("can't build the tray menu: {e}"))?;
    let mut tray: Option<TrayIcon> = None;
    let mut pending: Vec<PathBuf> = Vec::new();
    let mut t = Tracker::default();
    let mut reset_at: Option<Instant> = None;
    let mut error_until: Option<Instant> = None;
    // Pairing code shown in the title until it expires.
    let mut code: Option<(String, Instant)> = None;
    let mut receiving = 0usize;
    let mut peer: Option<String> = None;
    let mut next_peer_check = Instant::now();
    let mut visible = true;
    let mut shown = title_for(&DropState::Idle);
    let mut shown_tray: Option<(TrayState, Option<String>)> = None;

    event_loop.run(move |event, target, flow| {
        match event {
            // The tray must be created once the loop runs (GTK main thread).
            Event::NewEvents(StartCause::Init) => {
                // libappindicator-sys panics (not Err) when the library is missing at runtime.
                let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    TrayIconBuilder::new()
                        .with_menu(Box::new(menu.clone()))
                        .with_icon(tray_icon_for(TrayState::Unpaired))
                        .build()
                        .map_err(|e| e.to_string())
                }))
                .unwrap_or_else(|_| Err("tray library missing (install libayatana-appindicator3-1)".into()));
                match built {
                    Ok(i) => {
                        tray = Some(i);
                        // Tray-only app: no task list / taskbar entry. Without a tray the
                        // window stays in it, so a minimized window can't become unreachable.
                        let _ = window.set_skip_taskbar(true);
                    }
                    Err(e) => notify::error(&format!("no tray icon, the drop window's close button now quits: {e}")),
                }
            }
            Event::WindowEvent { event, .. } => match event {
                // Without a tray there would be no way to get the window back: close quits.
                WindowEvent::CloseRequested if tray.is_none() => {
                    *flow = ControlFlow::Exit;
                    return;
                }
                WindowEvent::CloseRequested => {
                    window.set_visible(false);
                    visible = false;
                    toggle.set_text("Show drop window");
                }
                // Windows reports minimize as a 0x0 resize: hide to the tray instead.
                WindowEvent::Resized(sz) if sz.width == 0 || sz.height == 0 => {
                    if tray.is_some() {
                        window.set_minimized(false);
                        window.set_visible(false);
                        visible = false;
                        toggle.set_text("Show drop window");
                    }
                }
                // Not user-resizable, so a resize is the WM/GTK settling the real size: re-place.
                WindowEvent::Resized(_) => place(&window, target),
                WindowEvent::HoveredFile(_) => t.hover = true,
                WindowEvent::HoveredFileCancelled => t.hover = false,
                WindowEvent::DroppedFile(p) => {
                    t.hover = false;
                    pending.push(p);
                }
                _ => {}
            },
            Event::UserEvent(UserEvent::Minimized) => {
                if tray.is_some() {
                    window.set_minimized(false);
                    window.set_visible(false);
                    visible = false;
                    toggle.set_text("Show drop window");
                }
            }
            Event::UserEvent(UserEvent::Hover(on)) => t.hover = on,
            Event::UserEvent(UserEvent::Dropped(files)) => {
                t.hover = false;
                pending.extend(files);
            }
            // End of an event batch: all files of one drop have arrived.
            Event::MainEventsCleared if !pending.is_empty() => {
                let files = std::mem::take(&mut pending);
                t.start(files.len());
                reset_at = None;
                let proxy = proxy.clone();
                std::thread::spawn(move || {
                    let res = transfer::check_files(&files)
                        .and_then(|_| transfer::send_files(&transfer::only_partner(config::load_partners()?, None)?, &files));
                    let _ = proxy.send_event(UserEvent::SendDone(files.len(), res));
                });
            }
            Event::UserEvent(UserEvent::SendDone(n, res)) => {
                match &res {
                    Ok(m) => notify::info(m),
                    Err(e) => {
                        notify::error(e);
                        error_until = Some(Instant::now() + ERROR_FOR);
                    }
                }
                t.finish(n, res.is_ok());
                next_peer_check = Instant::now();
                if t.in_flight == 0 {
                    reset_at = Some(Instant::now() + RESET_AFTER);
                }
            }
            Event::UserEvent(UserEvent::Fatal(e)) => {
                notify::error(&e);
                std::process::exit(1);
            }
            Event::UserEvent(UserEvent::Listener(e)) => match e {
                transfer::Event::Receiving(_) => receiving += 1,
                transfer::Event::Received(r) => {
                    receiving = receiving.saturating_sub(1);
                    next_peer_check = Instant::now();
                    notify::received(&r)
                }
                transfer::Event::Paired(name) => {
                    code = None;
                    next_peer_check = Instant::now();
                    notify::info(&format!("Paired with {name}"));
                }
                transfer::Event::PairFailed(e) => {
                    code = None;
                    notify::error(&e);
                    error_until = Some(Instant::now() + ERROR_FOR);
                }
                transfer::Event::Error(e) => {
                    // ponytail: an Error without a Receiving (accept failure) can end a parallel receive's blue early.
                    receiving = receiving.saturating_sub(1);
                    notify::error(&e);
                    error_until = Some(Instant::now() + ERROR_FOR);
                }
                // A stranger knocking is logged by the listener's caller in the CLI; here it is only noise.
                transfer::Event::Refused(ip) => eprintln!("refused connection from {ip}: not a paired PC"),
            },
            Event::UserEvent(UserEvent::Menu(id)) => {
                if id == *quit.id() {
                    *flow = ControlFlow::Exit;
                    return;
                } else if id == *toggle.id() {
                    visible = !visible;
                    window.set_visible(visible);
                    toggle.set_text(if visible { "Hide drop window" } else { "Show drop window" });
                } else if id == *pair_item.id() {
                    let c = pair::new_code();
                    let now = Instant::now();
                    *pairing.lock().unwrap_or_else(|e| e.into_inner()) = Some((c.clone(), now));
                    notify::pair_command(&format!(
                        "synergy-tools pair {} {}",
                        config::local_address(),
                        pair::show_code(&c)
                    ));
                    code = Some((c, now));
                }
            }
            Event::NewEvents(_) => {
                let now = Instant::now();
                if reset_at.is_some_and(|d| now >= d) {
                    reset_at = None;
                    t.result = None;
                }
                if error_until.is_some_and(|d| now >= d) {
                    error_until = None;
                }
                if code.as_ref().is_some_and(|(_, at)| at.elapsed() >= transfer::PAIR_WINDOW) {
                    code = None;
                }
                if now >= next_peer_check {
                    // Names of all partners, or None when there are none.
                    peer = config::load_partners()
                        .ok()
                        .filter(|ps| !ps.is_empty())
                        .map(|ps| ps.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", "));
                    next_peer_check = now + PEER_CHECK;
                }
            }
            _ => {}
        }
        let st = t.state();
        let title = match (&code, &st) {
            (Some((c, _)), DropState::Idle | DropState::Done(_)) => format!("Pairing code {}", pair::show_code(c)),
            _ => title_for(&st),
        };
        if title != shown {
            window.set_title(&title);
            shown = title;
        }
        let ts = tray_state(peer.is_some(), error_until.is_some(), t.in_flight > 0 || receiving > 0);
        if shown_tray.as_ref() != Some(&(ts, peer.clone())) {
            status.set_text(status_text(peer.as_deref()));
            if let Some(i) = &tray {
                let _ = i.set_icon(Some(tray_icon_for(ts)));
                let _ = i.set_tooltip(Some(tooltip(peer.as_deref())));
            }
            shown_tray = Some((ts, peer.clone()));
        }
        // Computed after handling, so a timer armed above takes effect at once.
        let wake = [reset_at, error_until, code.as_ref().map(|(_, at)| *at + transfer::PAIR_WINDOW), Some(next_peer_check)].into_iter().flatten().min();
        *flow = wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn uri_list() {
        use std::os::unix::ffi::OsStrExt;
        let l = parse_uri_list(
            "# comment\r\nfile:///a/b%20c.txt\r\n\r\nfile://localhost/x/y\nhttp://h/z\nfile://other/q\nfile:///bad%FF\n",
        );
        assert_eq!(l.len(), 3);
        assert_eq!(l[0], PathBuf::from("/a/b c.txt"));
        assert_eq!(l[1], PathBuf::from("/x/y"));
        assert_eq!(l[2].as_os_str().as_bytes(), b"/bad\xff");
    }

    #[test]
    fn tray_state_precedence() {
        use TrayState::*;
        assert_eq!(tray_state(true, true, true), Error);
        assert_eq!(tray_state(false, true, false), Error);
        assert_eq!(tray_state(false, false, true), Unpaired);
        assert_eq!(tray_state(true, false, true), Busy);
        assert_eq!(tray_state(true, false, false), Idle);
    }

    #[test]
    fn icon_size() {
        assert_eq!(icon_rgba(TrayState::Idle).len(), 32 * 32 * 4);
    }

    #[test]
    fn icon_states_differ() {
        let all = [TrayState::Unpaired, TrayState::Idle, TrayState::Busy, TrayState::Error];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(icon_rgba(*a), icon_rgba(*b));
            }
        }
        let px = icon_rgba(TrayState::Idle);
        assert_eq!(px[3], 0, "corner is transparent");
        assert_eq!(px[(16 * 32 + 16) * 4 + 3], 255, "centre is opaque");
    }

    #[test]
    fn titles() {
        assert_eq!(title_for(&DropState::Idle), "Drop files to send");
        assert_eq!(title_for(&DropState::Hover), "Release to send");
        assert_eq!(title_for(&DropState::Sending(1)), "Sending 1 file…");
        assert_eq!(title_for(&DropState::Sending(2)), "Sending 2 files…");
        assert_eq!(title_for(&DropState::Done(true)), "Sent ✓");
        assert_eq!(title_for(&DropState::Done(false)), "Not sent");
    }

    #[test]
    fn overlapping_sends() {
        let mut t = Tracker::default();
        t.start(2); // drop A
        t.start(1); // drop B
        t.finish(2, false); // A fails
        assert_eq!(t.state(), DropState::Sending(1));
        t.hover = true;
        t.hover = false; // hover-cancel while sending
        assert_eq!(t.state(), DropState::Sending(1));
        t.finish(1, true); // B ok, but A failed: group fails
        assert_eq!(t.state(), DropState::Done(false));
        t.start(1);
        t.finish(1, true); // new group starts clean
        assert_eq!(t.state(), DropState::Done(true));
        t.hover = true;
        assert_eq!(t.state(), DropState::Hover);
        t.hover = false;
        assert_eq!(t.state(), DropState::Done(true)); // restored, not Idle
    }
}
