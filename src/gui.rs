//! Drop window: a small always-on-top window; files dropped on it are sent to the paired PC.
//! State is shown in the window title (no drawing crate).

use crate::{config, notify, transfer};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;

/// Requested size; the WM/GTK may enlarge it (200x200 on Openbox), so placement uses the real size.
const SIZE: f64 = 160.0;
/// Gap kept free at the bottom-right (taskbar / panel).
const MARGIN: f64 = 48.0;
const RESET_AFTER: Duration = Duration::from_secs(3);

/// Messages from worker threads to the event loop (later tasks add variants).
pub enum UserEvent {
    SendDone(usize, Result<String, String>),
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

pub fn run() -> Result<(), String> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window = WindowBuilder::new()
        .with_title(title_for(&DropState::Idle))
        .with_inner_size(LogicalSize::new(SIZE, SIZE))
        .with_resizable(false)
        .with_always_on_top(true)
        .build(&event_loop)
        .map_err(|e| format!("can't open the drop window: {e}"))?;
    let mut pending: Vec<PathBuf> = Vec::new();
    let mut t = Tracker::default();
    let mut reset_at: Option<Instant> = None;
    let mut shown = title_for(&DropState::Idle);

    event_loop.run(move |event, target, flow| {
        match event {
            Event::WindowEvent { event, .. } => match event {
                // Task 8 (tray) switches this to hiding the window.
                WindowEvent::CloseRequested => {
                    *flow = ControlFlow::Exit;
                    return;
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
                    Err(e) => notify::error(e),
                }
                t.finish(n, res.is_ok());
                if t.in_flight == 0 {
                    reset_at = Some(Instant::now() + RESET_AFTER);
                }
            }
            Event::NewEvents(_) if reset_at.is_some_and(|d| Instant::now() >= d) => {
                reset_at = None;
                t.result = None;
            }
            _ => {}
        }
        let title = title_for(&t.state());
        if title != shown {
            window.set_title(&title);
            shown = title;
        }
        // Computed after handling, so a timer armed above takes effect at once.
        *flow = reset_at.map_or(ControlFlow::Wait, ControlFlow::WaitUntil);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
