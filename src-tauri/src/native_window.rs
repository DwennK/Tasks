//! Keep the native macOS window controls inside the 62-point navigation rail.

#[cfg(target_os = "macos")]
pub fn layout_controls(window: &tauri::Window) {
    if window.label() != "main" {
        return;
    }
    let target = window.clone();
    let _ = window.run_on_main_thread(move || {
        use objc2_app_kit::{NSWindow, NSWindowButton, NSWindowStyleMask};

        let Ok(pointer) = target.ns_window() else {
            return;
        };
        // Tauri owns this NSWindow; the retained window handle keeps it alive,
        // and all AppKit access happens on the main thread.
        let native = unsafe { &*pointer.cast::<NSWindow>() };
        if native.styleMask().contains(NSWindowStyleMask::FullScreen) {
            return;
        }
        // Preserve the system buttons, their size, accessibility and actions.
        // Only replace the wider macOS spacing with the compact 18-point pitch.
        for (index, kind) in [
            NSWindowButton::CloseButton,
            NSWindowButton::MiniaturizeButton,
            NSWindowButton::ZoomButton,
        ]
        .into_iter()
        .enumerate()
        {
            if let Some(button) = native.standardWindowButton(kind) {
                let mut origin = button.frame().origin;
                origin.x = 7.0 + index as f64 * 18.0;
                button.setFrameOrigin(origin);
            }
        }
    });
}

#[cfg(not(target_os = "macos"))]
pub fn layout_controls(_window: &tauri::Window) {}
