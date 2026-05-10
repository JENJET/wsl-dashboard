use crate::{AppInfo, AppState, AppWindow};
use slint::ComponentHandle;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

// Automatically check for updates and expiration at startup
pub fn spawn_check_task(app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    let ah = app_handle.clone();
    let app_state_check = app_state.clone();

    tokio::spawn(async move {
        let current_v = env!("CARGO_PKG_VERSION");
        // Wait a moment before checking to avoid affecting startup speed
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // Read last popup time and check update interval
        let (last_check_time, check_update_days, timezone, is_silent_mode) = {
            let state = app_state_check.lock().await;
            let settings = state.config_manager.get_settings();
            let timezone = state.config_manager.get_config().system.timezone.clone();
            (
                settings.check_time.parse::<i64>().unwrap_or(0),
                settings.check_update as i64,
                timezone,
                state.is_silent_mode,
            )
        };

        if is_silent_mode {
            info!("Skipping startup checks (silent mode)");
            return;
        }

        let now_ms = chrono::Utc::now().timestamp_millis();
        let interval_ms: i64 = check_update_days * 24 * 60 * 60 * 1000;
        let should_check_update = (now_ms - last_check_time) >= interval_ms;

        info!(
            "Check-update: last={}, now={}, interval={}, should_check_update={}",
            last_check_time, now_ms, interval_ms, should_check_update
        );

        // If not time to check, skip both expiration and update checks
        if !should_check_update {
            info!("Skipping startup checks (interval not reached)");
            return;
        }

        // Check expiration first
        let expire_time_str = env!("APP_EXPIRE_TIME");
        let expire_time: i64 = expire_time_str.parse().unwrap_or(0);

        if expire_time > 0 {
            info!(
                "Checking expiration first. App expire time: {}",
                expire_time
            );
            let timezone_inner = timezone.clone();
            let now = tokio::task::spawn_blocking(move || {
                crate::utils::time::standard_time(&timezone_inner)
            })
            .await
            .unwrap_or(0);

            info!("Current standard time: {}", now);

            if now > expire_time {
                let ah_c = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_c.upgrade() {
                        app.set_show_expire_dialog(true);
                    }
                });
                // Update check-update timestamp
                let mut state = app_state_check.lock().await;
                let _ = state.config_manager.update_check_time();
                info!("App expired! Skipping update check.");
                return;
            }
        }

        // If not expired, then check for updates
        let ah_c = ah.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = ah_c.upgrade() {
                app.global::<AppInfo>().set_checking_update(true);
            }
        });

        match crate::app::updater::check_update(current_v, &timezone).await {
            Ok(result) => {
                // Update status in AppInfo regardless of popup (used by About page)
                let has_update = result.has_update;
                let latest_version = result.latest_version.clone();

                let ah_c = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_c.upgrade() {
                        app.global::<AppInfo>().set_checking_update(false);
                        if has_update {
                            app.global::<AppInfo>().set_has_update(true);
                            app.global::<AppInfo>()
                                .set_latest_version(latest_version.into());
                        }
                    }
                });

                // Only show popup if there's an update (should_check_update is already true here)
                if has_update {
                    let ah_c = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_c.upgrade() {
                            app.set_show_update_dialog(true);
                        }
                    });
                    // Update check-update timestamp
                    let mut state = app_state_check.lock().await;
                    let _ = state.config_manager.update_check_time();
                }
            }
            Err(e) => {
                warn!("Auto check update failed: {}", e);
                let ah_c = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_c.upgrade() {
                        app.global::<AppInfo>().set_checking_update(false);
                    }
                });
            }
        }
    });
}
