use crate::config::models::CachedDistro;
use crate::ui::data::refresh_distros_ui;
use crate::{AppState, AppWindow};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

// Start WSL status monitoring task
pub fn spawn_wsl_monitor(app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let ah = app_handle.clone();
            let as_ptr = app_state.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah.upgrade() {
                    let is_visible = app.get_is_window_visible();
                    // Only refresh if the Home tab (index 0) is selected to save resources
                    if app.get_selected_tab() == 0 {
                        let as_ptr_res = as_ptr.clone();
                        tokio::spawn(async move {
                            if crate::ui::data::should_refresh_wsl("periodic trigger", is_visible) {
                                let dashboard = {
                                    let state = as_ptr_res.lock().await;
                                    state.wsl_dashboard.clone()
                                };
                                let _ = dashboard.refresh_distros().await;
                            }
                        });
                    }
                }
            });
        }
    });
}

// Start USB status monitoring task
pub fn spawn_usb_monitor(app_handle: slint::Weak<AppWindow>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let ah = app_handle.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah.upgrade() {
                    // Only refresh if the USB tab (index 2) is selected to save resources
                    if app.get_selected_tab() == 2 {
                        app.invoke_refresh_usb(false);
                    }
                }
            });
        }
    });
}

// Listen for distribution state changes and automatically update UI
pub fn spawn_state_listener(app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    tokio::spawn(async move {
        let mut last_refresh = std::time::Instant::now();
        const MIN_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(1000);

        loop {
            {
                let lock_timeout = std::time::Duration::from_millis(500);
                let state_changed = match tokio::time::timeout(lock_timeout, app_state.lock()).await
                {
                    Ok(state) => state.wsl_dashboard.state_changed().clone(),
                    Err(_) => {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        continue;
                    }
                };
                state_changed.notified().await;
            }

            // Debounce: limit minimum refresh interval to reduce memory pressure
            let now = std::time::Instant::now();
            let elapsed = now.duration_since(last_refresh);
            if elapsed < MIN_REFRESH_INTERVAL {
                tokio::time::sleep(MIN_REFRESH_INTERVAL - elapsed).await;
            }

            debug!("WSL state changed, updating UI...");
            let _ = refresh_distros_ui(app_handle.clone(), app_state.clone()).await;

            // Save updated distro list to cache for fast startup next time
            let app_state_for_cache = app_state.clone();
            tokio::spawn(async move {
                let lock_timeout = std::time::Duration::from_millis(500);
                let (distros, config_manager) =
                    match tokio::time::timeout(lock_timeout, app_state_for_cache.lock()).await {
                        Ok(state) => (
                            state.wsl_dashboard.get_distros().await,
                            state.config_manager.clone(),
                        ),
                        Err(_) => return,
                    };

                let cached: Vec<CachedDistro> = distros
                    .into_iter()
                    .map(|d| CachedDistro {
                        name: d.name,
                        status: format!("{:?}", d.status),
                        version: format!("{:?}", d.version),
                        is_default: d.is_default,
                    })
                    .collect();

                let _ = config_manager.update_cached_distros(cached);
                debug!("WSL distro list cache updated.");
            });

            last_refresh = std::time::Instant::now();
        }
    });
}

// Processing after application exit
pub async fn handle_app_exit(app: &AppWindow, app_state: &Arc<Mutex<AppState>>) {
    let auto_shutdown = app.get_auto_shutdown();
    if auto_shutdown {
        debug!("Auto-shutdown on exit is enabled, shutting down WSL...");
        let manager = {
            let state = app_state.lock().await;
            state.wsl_dashboard.clone()
        };
        manager.shutdown_wsl().await;
        debug!("WSL shut down completed");
    }
}
