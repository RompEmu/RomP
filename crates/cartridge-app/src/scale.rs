use slint::platform::WindowEvent;
use slint::winit_030::winit::window::WindowId;
use slint::winit_030::WinitWindowAccessor;
use slint::ComponentHandle;
use std::cell::{Cell, RefCell};
use std::time::Duration;

type Target = Box<dyn Fn(Option<WindowId>) -> bool>;

thread_local! {
    static FACTOR: Cell<f32> = const { Cell::new(1.0) };
    static TARGETS: RefCell<Vec<Target>> = RefCell::default();
}

pub fn set_factor(factor: f32) {
    FACTOR.set(factor);
    refresh(None);
}

pub fn track<C: ComponentHandle + 'static>(component: &C) {
    apply(component.window());
    let weak = component.as_weak();
    TARGETS.with_borrow_mut(|targets| {
        targets.push(Box::new(move |only| {
            let Some(component) = weak.upgrade() else {
                return false;
            };
            let window = component.window();
            if only.is_none() || crate::mouse::window_id(window) == only {
                apply(window);
            }
            true
        }))
    });
}

pub fn window_event() {
    refresh(None);
}

pub fn system_changed(id: WindowId) {
    slint::Timer::single_shot(Duration::ZERO, move || refresh(Some(id)));
}

fn refresh(only: Option<WindowId>) {
    let targets = TARGETS.take();
    let mut alive: Vec<Target> = targets.into_iter().filter(|t| t(only)).collect();
    TARGETS.with_borrow_mut(|targets| {
        alive.append(targets);
        *targets = alive;
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
