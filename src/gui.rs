//! Drop window: a small always-on-top window; files dropped on it are sent to the paired PC.
//! State is shown in the window title (no drawing crate).

use crate::{config, notify, pair, transfer};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
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
    SendDone(usize, Result<String, String>),
    Listener(transfer::Event),
    Menu(MenuId),
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

fn tray_icon_for(s: TrayState) -> Icon {
    Icon::from_rgba(icon_rgba(s), 32, 32).expect("32x32 RGBA")
}

fn tooltip(paired: Option<&str>) -> String {
    match paired {
        Some(h) => format!("synergy-dragndrop: paired with {h}"),
        None => "synergy-dragndrop: not paired".into(),
    }
}

pub fn run() -> Result<(), String> {
    // Single instance: the port is the lock.
    let dir = dirs::download_dir().ok_or_else(|| "can't find your Downloads folder".to_string())?;
    let listener = transfer::Listener::bind(crate::PORT, dir).inspect_err(|e| notify::error(e))?;
    let pairing = listener.pairing.clone();

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    {
        let proxy = proxy.clone();
        std::thread::spawn(move || {
            listener.run(move |e| {
                let _ = proxy.send_event(UserEvent::Listener(e));
            })
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
    let (toggle, pair_item, quit) = (
        MenuItem::new("Hide drop window", true, None),
        MenuItem::new("Pair…", true, None),
        MenuItem::new("Quit", true, None),
    );
    let menu = Menu::new();
    menu.append_items(&[&toggle, &pair_item, &quit]).map_err(|e| format!("can't build the tray menu: {e}"))?;
    let mut tray: Option<TrayIcon> = None;
    let mut pending: Vec<PathBuf> = Vec::new();
    let mut t = Tracker::default();
    let mut reset_at: Option<Instant> = None;
    let mut error_until: Option<Instant> = None;
    // Pairing code shown in the title until it expires.
    let mut code: Option<(String, Instant)> = None;
    let mut visible = true;
    let mut shown = title_for(&DropState::Idle);
    let mut shown_tray: Option<(TrayState, Option<String>)> = None;

    event_loop.run(move |event, target, flow| {
        match event {
            // The tray must be created once the loop runs (GTK main thread).
            Event::NewEvents(StartCause::Init) => {
                match TrayIconBuilder::new().with_menu(Box::new(menu.clone())).with_icon(tray_icon_for(TrayState::Unpaired)).build() {
                    Ok(i) => tray = Some(i),
                    Err(e) => notify::error(&format!("can't create the tray icon: {e}")),
                }
            }
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    window.set_visible(false);
                    visible = false;
                    toggle.set_text("Show drop window");
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
            // End of an event batch: all files of one drop have arrived.
            Event::MainEventsCleared if !pending.is_empty() => {
                let files = std::mem::take(&mut pending);
                t.start(files.len());
                reset_at = None;
                let proxy = proxy.clone();
                std::thread::spawn(move || {
                    let res = transfer::check_files(&files)
                        .and_then(|_| transfer::send_files(&config::load_peer()?, &files));
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
                if t.in_flight == 0 {
                    reset_at = Some(Instant::now() + RESET_AFTER);
                }
            }
            Event::UserEvent(UserEvent::Listener(e)) => match e {
                transfer::Event::Received(r) => notify::received(&r),
                transfer::Event::Paired(ip) => {
                    code = None;
                    notify::info(&format!("Paired with {ip}"));
                }
                transfer::Event::PairFailed(e) => {
                    code = None;
                    notify::error(&e);
                    error_until = Some(Instant::now() + ERROR_FOR);
                }
                transfer::Event::Error(e) => {
                    notify::error(&e);
                    error_until = Some(Instant::now() + ERROR_FOR);
                }
                // A stranger knocking is logged by the listener's caller in the CLI; here it is only noise.
                transfer::Event::Refused(ip) => eprintln!("refused connection from {ip}: not the paired PC"),
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
                    notify::info(&format!(
                        "On the other PC run: synergy-dragndrop pair {} {}",
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
        let peer = config::load_peer().ok().map(|p| p.host);
        let ts = match (&peer, error_until, t.in_flight) {
            (_, Some(_), _) => TrayState::Error,
            (None, ..) => TrayState::Unpaired,
            (_, _, 0) => TrayState::Idle,
            _ => TrayState::Busy,
        };
        if shown_tray.as_ref() != Some(&(ts, peer.clone())) {
            if let Some(i) = &tray {
                let _ = i.set_icon(Some(tray_icon_for(ts)));
                let _ = i.set_tooltip(Some(tooltip(peer.as_deref())));
            }
            shown_tray = Some((ts, peer));
        }
        // Computed after handling, so a timer armed above takes effect at once.
        let wake = [reset_at, error_until, code.as_ref().map(|(_, at)| *at + transfer::PAIR_WINDOW)].into_iter().flatten().min();
        *flow = wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
