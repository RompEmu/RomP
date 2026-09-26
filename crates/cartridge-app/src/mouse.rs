use slint::winit_030::winit::event::{DeviceEvent, DeviceId};
use slint::winit_030::winit::event_loop::ActiveEventLoop;
use slint::winit_030::winit::window::CursorGrabMode;
use slint::winit_030::{CustomApplicationHandler, EventResult, WinitWindowAccessor};
use std::cell::{Cell, RefCell};

#[derive(Debug, Default, PartialEq)]
pub struct Motion {
    x: f64,
    y: f64,
}

impl Motion {
    pub fn add(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
    }

    pub fn take(&mut self) -> (i16, i16) {
        fn split(v: &mut f64) -> i16 {
            let whole = v.trunc();
            let clamped = whole.clamp(f64::from(i16::MIN), f64::from(i16::MAX));
            *v = if clamped == whole { *v - whole } else { 0.0 };
            clamped as i16
        }
        (split(&mut self.x), split(&mut self.y))
    }
}

thread_local! {
    static MOTION: RefCell<Motion> = RefCell::default();
    static CAPTURED: Cell<bool> = const { Cell::new(false) };
}

pub struct RawMouse;

impl CustomApplicationHandler for RawMouse {
    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) -> EventResult {
        if let DeviceEvent::MouseMotion { delta } = event {
            if CAPTURED.get() {
                MOTION.with_borrow_mut(|m| m.add(delta.0, delta.1));
            }
        }
        EventResult::Propagate
    }
}

pub fn take_motion() -> (i16, i16) {
    MOTION.with_borrow_mut(Motion::take)
}

pub fn capture(window: &slint::Window, on: bool) {
    window.with_winit_window(|w| {
        if on {
            let grabbed = w
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| w.set_cursor_grab(CursorGrabMode::Confined));
            if let Err(e) = grabbed {
                tracing::warn!("could not capture the mouse: {e}");
            }
        } else {
            let _ = w.set_cursor_grab(CursorGrabMode::None);
        }
        w.set_cursor_visible(!on);
    });
    CAPTURED.set(on);
    MOTION.with_borrow_mut(|m| *m = Motion::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_is_reported_in_whole_steps_and_keeps_the_remainder() {
        let mut m = Motion::default();
        m.add(1.4, -0.6);
        m.add(0.4, -0.6);
        assert_eq!(m.take(), (1, -1));
        m.add(0.3, 0.0);
        assert_eq!(m.take(), (1, 0));
        assert_eq!(m.take(), (0, 0));
        m.add(1e9, -1e9);
        assert_eq!(m.take(), (i16::MAX, i16::MIN));
    }
}
