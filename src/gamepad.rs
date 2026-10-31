//! Controller input. gilrs runs on its own thread and queues simple UI actions for the window to drain.
use gilrs::{Axis, Button, EventType, Gilrs};
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
}

pub type Queue = Arc<Mutex<Vec<Pad>>>;

/// Starts listening for controllers. If gilrs can't start (no input backend), the queue just stays empty.
pub fn spawn() -> Queue {
    let queue: Queue = Arc::default();
    let out = queue.clone();
    std::thread::spawn(move || {
        let Ok(mut gilrs) = Gilrs::new() else { return };
        let mut stick = Stick::default();
        loop {
            let Some(event) = gilrs.next_event_blocking(None) else { continue };
            if let Some(action) = map(event.event, &mut stick) {
                out.lock().unwrap().push(action);
            }
        }
    });
    queue
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
