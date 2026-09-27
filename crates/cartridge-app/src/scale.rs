use crate::AppWindow;
use slint::platform::WindowEvent;
use slint::winit_030::winit::window::WindowId;
use slint::winit_030::WinitWindowAccessor;
use slint::ComponentHandle;
use std::cell::{Cell, RefCell};
use std::time::Duration;

thread_local! {
    static FACTOR: Cell<f32> = const { Cell::new(1.0) };
    static WINDOW: RefCell<Option<slint::Weak<AppWindow>>> = const { RefCell::new(None) };
}

pub fn set(ui: &AppWindow, factor: f32) {
    FACTOR.set(factor);
    WINDOW.set(Some(ui.as_weak()));
    apply(ui.window());
}

pub fn system_changed(id: WindowId) {
    slint::Timer::single_shot(Duration::ZERO, move || {
        let Some(ui) = WINDOW.with_borrow(|w| w.as_ref().and_then(|w| w.upgrade())) else {
            return;
        };
        if crate::mouse::window_id(ui.window()) == Some(id) {
            apply(ui.window());
        }
    });
}

fn apply(window: &slint::Window) {
    let Some(system) = window.with_winit_window(|w| w.scale_factor() as f32) else {
        return;
    };
    let scale = system * FACTOR.get();
    if (window.scale_factor() - scale).abs() < f32::EPSILON {
        return;
    }
    let physical = window.size();
    window.dispatch_event(WindowEvent::ScaleFactorChanged {
        scale_factor: scale,
    });
    window.dispatch_event(WindowEvent::Resized {
        size: physical.to_logical(scale),
    });
    window.request_redraw();
}
