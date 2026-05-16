use crate::{AppState, AppWindow};
use slint::ComponentHandle;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

pub fn setup(app: &AppWindow, app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    // System window close button (X) and Alt+F4
    let ah = app_handle.clone();
    let state = app_state.clone();
    app.window().on_close_requested(move || {
        if let Some(app) = ah.upgrade() {
            if app.get_tray_close_to_tray() {
                info!("Close requested, hiding window to tray...");
                app.set_is_window_visible(false);
                crate::app::window::set_skip_taskbar(&app, true);
                return slint::CloseRequestResponse::KeepWindowShown;
            }

            let is_blocked = state
                .try_lock()
                .map(|s| s.wsl_dashboard.is_manual_operation())
                .unwrap_or(false);

            if is_blocked {
                info!("Close blocked: manual operation in progress");
                app.set_show_busy_dialog(true);
                return slint::CloseRequestResponse::KeepWindowShown;
            }

            info!("Close requested, quitting...");
            let _ = slint::quit_event_loop();
        }
        slint::CloseRequestResponse::HideWindow
    });

    // Custom title bar close button
    let ah = app_handle.clone();
    let state = app_state.clone();
    app.on_window_close(move || {
        if let Some(app) = ah.upgrade() {
            if app.get_tray_close_to_tray() {
                info!("Title bar close clicked, hiding window to tray...");
                app.set_is_window_visible(false);
                crate::app::window::set_skip_taskbar(&app, true);
                return;
            }

            let is_blocked = state
                .try_lock()
                .map(|s| s.wsl_dashboard.is_manual_operation())
                .unwrap_or(false);

            if is_blocked {
                info!("Title bar close blocked: manual operation in progress");
                app.set_show_busy_dialog(true);
                return;
            }

            info!("Title bar close clicked, quitting...");
            let _ = slint::quit_event_loop();
        }
    });

    let ah = app_handle.clone();
    app.on_window_minimize(move || {
        if let Some(app) = ah.upgrade() {
            app.window().set_minimized(true);
        }
    });

    let ah = app_handle.clone();
    app.on_window_maximize(move || {
        if let Some(app) = ah.upgrade() {
            let is_max = app.get_is_maximized();
            app.set_is_maximized(!is_max);
            app.window().set_maximized(!is_max);
        }
    });

    // ensure_card_visible is now handled entirely in Slint (main_view.slint)
    // The Slint-side Timer waits for the expand animation (200ms) to complete,
    // then reads the actual Flickable dimensions and sets list_scroll_y accordingly.
    // This ensures the scroll position is calculated after the height animation finishes.

    // Busy dialog: confirm → force quit immediately
    let ah = app_handle.clone();
    app.on_busy_confirm(move || {
        info!("Busy confirm: force quitting...");
        if let Some(app) = ah.upgrade() {
            app.set_show_busy_dialog(false);
        }
        let _ = slint::quit_event_loop();
    });

    // Busy dialog: cancel → go back to normal
    let ah = app_handle.clone();
    app.on_busy_cancel(move || {
        info!("Busy cancel: close cancelled");
        if let Some(app) = ah.upgrade() {
            app.set_show_busy_dialog(false);
        }
    });

    let ah = app_handle.clone();
    app.on_window_drag_delta(move |dx, dy| {
        if let Some(app) = ah.upgrade() {
            let pos = app.window().position();
            app.window().set_position(slint::PhysicalPosition::new(
                pos.x + dx as i32,
                pos.y + dy as i32,
            ));
        }
    });
}
