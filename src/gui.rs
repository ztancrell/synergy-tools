//! Drop window: a small always-on-top window; files dropped on it are sent to the paired PC.
//! State is shown in the window title (no drawing crate).

use crate::{config, notify, transfer};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;

const SIZE: f64 = 160.0;
/// Gap kept free at the bottom-right (taskbar / panel).
const MARGIN: f64 = 48.0;
const RESET_AFTER: Duration = Duration::from_secs(3);

/// Messages from worker threads to the event loop (later tasks add variants).
pub enum UserEvent {
    SendDone(Result<String, String>),
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

pub fn run() -> Result<(), String> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let mut builder = WindowBuilder::new()
        .with_title(title_for(&DropState::Idle))
        .with_inner_size(LogicalSize::new(SIZE, SIZE))
        .with_resizable(false)
        .with_always_on_top(true);
    if let Some(m) = event_loop.primary_monitor() {
        let s = m.scale_factor();
        let (p, sz) = (m.position(), m.size());
        let x = p.x as f64 + sz.width as f64 - (SIZE + MARGIN) * s;
        let y = p.y as f64 + sz.height as f64 - (SIZE + MARGIN) * s;
        builder = builder.with_position(PhysicalPosition::new(x, y));
    }
    let window = builder.build(&event_loop).map_err(|e| format!("can't open the drop window: {e}"))?;

    let mut pending: Vec<PathBuf> = Vec::new();
    let mut reset_at: Option<Instant> = None;
    let set = move |s: DropState| window.set_title(&title_for(&s));

    event_loop.run(move |event, _, flow| {
        *flow = match reset_at {
            Some(t) => ControlFlow::WaitUntil(t),
            None => ControlFlow::Wait,
        };
        match event {
            Event::WindowEvent { event, .. } => match event {
                // Task 8 (tray) switches this to hiding the window.
                WindowEvent::CloseRequested => *flow = ControlFlow::Exit,
                WindowEvent::HoveredFile(_) => set(DropState::Hover),
                WindowEvent::HoveredFileCancelled => set(DropState::Idle),
                WindowEvent::DroppedFile(p) => pending.push(p),
                _ => {}
            },
            // End of an event batch: all files of one drop have arrived.
            Event::MainEventsCleared if !pending.is_empty() => {
                let files = std::mem::take(&mut pending);
                set(DropState::Sending(files.len()));
                reset_at = None;
                let proxy = proxy.clone();
                std::thread::spawn(move || {
                    let res = transfer::check_files(&files)
                        .and_then(|_| transfer::send_files(&config::load_peer()?, &files));
                    let _ = proxy.send_event(UserEvent::SendDone(res));
                });
            }
            Event::UserEvent(UserEvent::SendDone(res)) => {
                match &res {
                    Ok(m) => notify::info(m),
                    Err(e) => notify::error(e),
                }
                set(DropState::Done(res.is_ok()));
                reset_at = Some(Instant::now() + RESET_AFTER);
            }
            Event::NewEvents(_) if reset_at.is_some_and(|t| Instant::now() >= t) => {
                reset_at = None;
                set(DropState::Idle);
            }
            _ => {}
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles() {
        assert_eq!(title_for(&DropState::Idle), "Drop files to send");
        assert_eq!(title_for(&DropState::Hover), "Release to send");
        assert_eq!(title_for(&DropState::Sending(2)), "Sending 2 files…");
        assert_eq!(title_for(&DropState::Done(true)), "Sent ✓");
        assert_eq!(title_for(&DropState::Done(false)), "Not sent");
    }
}
