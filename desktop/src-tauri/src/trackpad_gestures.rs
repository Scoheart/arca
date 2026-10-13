//! Preserve macOS finger/momentum phases that DOM WheelEvent does not expose.

#[tauri::command]
pub fn trackpad_gestures_available() -> bool {
    #[cfg(target_os = "macos")]
    { return macos::available(); }
    #[cfg(not(target_os = "macos"))]
    { false }
}

#[cfg(target_os = "macos")]
mod macos {
    use block2::RcBlock;
    use objc2::{rc::Retained, runtime::AnyObject};
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventPhase, NSView};
    use serde::Serialize;
    use std::{cell::{Cell, RefCell}, ptr::NonNull, sync::atomic::{AtomicBool, Ordering}};
    use tauri::{Emitter, WebviewWindow};

    static AVAILABLE: AtomicBool = AtomicBool::new(false);
    thread_local! {
        // AppKit owns the callback; retain its token until the app exits.
        static MONITOR: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    }

    #[derive(Clone, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Scroll {
        gesture: u64,
        phase: usize,
        momentum: bool,
        delta_x: f64,
        delta_y: f64,
        x: f64,
        y: f64,
    }

    pub fn available() -> bool { AVAILABLE.load(Ordering::Relaxed) }

    pub fn install(window: WebviewWindow) -> tauri::Result<()> {
        let events = window.clone();
        window.with_webview(move |webview| {
            // Tauri supplies a live WKWebView (an NSView subclass) on the main
            // thread. The local monitor also runs on that same thread.
            let Some(view) = (unsafe { Retained::retain(webview.inner().cast::<NSView>()) }) else { return; };
            let gesture = Cell::new(0_u64);
            let handler = RcBlock::new(move |pointer: NonNull<NSEvent>| {
                let event = unsafe { pointer.as_ref() };
                if !event.modifierFlags().contains(NSEventModifierFlags::Control) &&
                    view.window().is_some_and(|window| window.windowNumber() == event.windowNumber()) {
                    let phase = event.phase();
                    let momentum = event.momentumPhase() != NSEventPhase::None;
                    if phase.contains(NSEventPhase::Began) { gesture.set(gesture.get().wrapping_add(1)); }
                    let bounds = view.bounds();
                    let point = view.convertPoint_fromView(event.locationInWindow(), None);
                    if bounds.size.width > 0.0 && bounds.size.height > 0.0 {
                        let x = (point.x - bounds.origin.x) / bounds.size.width;
                        let y = (point.y - bounds.origin.y) / bounds.size.height;
                        // Match CSS coordinates even if the native view is not flipped.
                        let y = if view.isFlipped() { y } else { 1.0 - y };
                        let _ = events.emit("arca-trackpad-scroll", Scroll {
                            gesture: gesture.get(), phase: phase.0, momentum,
                            // AppKit's horizontal sign is opposite DOM WheelEvent.
                            delta_x: -event.scrollingDeltaX(), delta_y: -event.scrollingDeltaY(), x, y,
                        });
                    }
                }
                // Observe only; ordinary WebView scrolling is still delivered.
                pointer.as_ptr()
            });
            let token = unsafe {
                NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::ScrollWheel, &handler)
            };
            AVAILABLE.store(token.is_some(), Ordering::Relaxed);
            MONITOR.with(|monitor| {
                if let Some(previous) = monitor.replace(token) {
                    unsafe { NSEvent::removeMonitor(&previous); }
                }
            });
        })
    }

    pub fn stop() {
        AVAILABLE.store(false, Ordering::Relaxed);
        MONITOR.with(|monitor| {
            if let Some(token) = monitor.borrow_mut().take() {
                unsafe { NSEvent::removeMonitor(&token); }
            }
        });
    }
}

#[cfg(target_os = "macos")]
pub use macos::{install, stop};
