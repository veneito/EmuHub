//! Controller input. gilrs runs on its own thread and queues simple UI actions for the window to drain.
use gilrs::{Axis, Button, EventType, Gilrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pad {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
    PrevTab,
    NextTab,
    /// Start / Options / Menu button: opens a game's details.
    Options,
}

pub type Queue = Arc<Mutex<Vec<Pad>>>;

/// Whether the last controller used is a PlayStation one, so button hints show ✕ ◯ instead of A B.
pub static PLAYSTATION: AtomicBool = AtomicBool::new(false);

fn is_playstation(name: &str) -> bool {
    let name = name.to_lowercase();
    ["playstation", "dualshock", "dualsense", "ps4", "ps5", "sony", "wireless controller"].iter().any(|n| name.contains(n))
}

/// Starts listening for controllers. If gilrs can't start (no input backend), the queue just stays empty.
pub fn spawn() -> Queue {
    let queue: Queue = Arc::default();
    let out = queue.clone();
    std::thread::spawn(move || {
        let Ok(mut gilrs) = Gilrs::new() else { return };
        let mut stick = Stick::default();
        loop {
            let Some(event) = gilrs.next_event_blocking(None) else { continue };
            if let Some(pad) = gilrs.connected_gamepad(event.id) {
                PLAYSTATION.store(is_playstation(pad.name()), Ordering::Relaxed);
            }
            if let Some(action) = map(event.event, &mut stick) {
                out.lock().unwrap().push(action);
            }
        }
    });
    queue
}

/// Whether one of our windows is in front. GPUI's activation event alone missed RetroArch taking focus
/// on Windows, so there we ask the OS which process owns the foreground window.
#[cfg(target_os = "windows")]
pub fn app_in_front() -> bool {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetForegroundWindow() -> isize;
        fn GetWindowThreadProcessId(window: isize, pid: *mut u32) -> u32;
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut pid) };
    pid == std::process::id()
}

#[cfg(not(target_os = "windows"))]
pub fn app_in_front() -> bool {
    true // shortcut: relies on GPUI's activation event only; add an OS check if games still leak input on macOS/Linux.
}

/// Last left-stick position, so a push past the threshold fires once instead of every frame.
#[derive(Default)]
struct Stick {
    x: f32,
    y: f32,
}

const PUSH: f32 = 0.6;

fn map(event: EventType, stick: &mut Stick) -> Option<Pad> {
    match event {
        EventType::ButtonPressed(button, _) => match button {
            Button::DPadUp => Some(Pad::Up),
            Button::DPadDown => Some(Pad::Down),
            Button::DPadLeft => Some(Pad::Left),
            Button::DPadRight => Some(Pad::Right),
            Button::South => Some(Pad::Confirm),
            Button::East => Some(Pad::Back),
            // gilrs calls the shoulder bumpers LeftTrigger/RightTrigger (the analog ones are *Trigger2).
            Button::LeftTrigger => Some(Pad::PrevTab),
            Button::RightTrigger => Some(Pad::NextTab),
            Button::Start => Some(Pad::Options),
            _ => None,
        },
        EventType::AxisChanged(Axis::LeftStickX, v, _) => {
            let old = std::mem::replace(&mut stick.x, v);
            crossed(old, v).map(|positive| if positive { Pad::Right } else { Pad::Left })
        }
        EventType::AxisChanged(Axis::LeftStickY, v, _) => {
            let old = std::mem::replace(&mut stick.y, v);
            // gilrs reports up as positive Y.
            crossed(old, v).map(|positive| if positive { Pad::Up } else { Pad::Down })
        }
        _ => None,
    }
}

/// Some(true/false) when the stick just moved past the threshold in the positive/negative direction.
fn crossed(old: f32, new: f32) -> Option<bool> {
    if new > PUSH && old <= PUSH {
        Some(true)
    } else if new < -PUSH && old >= -PUSH {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stick_fires_once_per_push() {
        assert_eq!(crossed(0.0, 0.7), Some(true));
        assert_eq!(crossed(0.7, 0.9), None); // held: no repeat
        assert_eq!(crossed(0.9, 0.1), None); // released
        assert_eq!(crossed(0.1, -0.8), Some(false));
    }
}
