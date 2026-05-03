use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use crate::{AppWindow, AppState};
use tracing::debug;

/// Interval (seconds) for auto-refreshing WSL management data while the tab is active
const WSL_MANAGE_REFRESH_INTERVAL_SECS: u64 = 5;

pub fn setup(app: &AppWindow, app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    // Handle to the running WSL manage auto-refresh task (cancelled when leaving tab 4)
    let wsl_refresh_handle: Arc<Mutex<Option<JoinHandle<()>>>> = Arc::new(Mutex::new(None));

    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    let refresh_handle = wsl_refresh_handle.clone();
    app.on_select_tab(move |tab| {
        if let Some(app) = ah.upgrade() {
            app.set_selected_tab(tab);

            // Tab 1 is "Add an instance"
            if tab == 1 {
                let as_ptr = as_ptr.clone();
                let ah = ah.clone();
                slint::spawn_local(async move {
                    let state = as_ptr.lock().await;
                    let settings = state.config_manager.get_settings();
                    let location = settings.distro_location.clone();
                    drop(state);

                    if let Some(app) = ah.upgrade() {
                        app.set_distro_location(location.clone().into());

                        let current_name = app.get_new_instance_name().to_string();
                        let final_path = if !current_name.is_empty() {
                            std::path::Path::new(&location).join(&current_name).to_string_lossy().to_string()
                        } else {
                            location
                        };

                        app.set_new_instance_path(final_path.into());
                    }
                }).unwrap();
            }

            // Tab 2 is "USB Devices"
            if tab == 2 {
                app.invoke_refresh_usb(true);
            }

            // Tab 3 is "Network"
            if tab == 3 {
                app.invoke_check_network_task_status();
                let ah = ah.clone();
                let as_ptr = as_ptr.clone();
                tokio::spawn(async move {
                    crate::ui::handlers::network::refresh_network_view_data(ah, as_ptr).await;
                });
            }

            // Tab 4 is "WSL Management" — start periodic refresh
            if tab == 4 {
                let ah = ah.clone();
                let as_ptr = as_ptr.clone();
                let refresh_handle = refresh_handle.clone();

                // Cancel any existing refresh task immediately (sync)
                {
                    let handle = refresh_handle.try_lock();
                    if let Ok(mut h) = handle {
                        if let Some(task) = h.take() {
                            task.abort();
                            debug!("Cancelled previous WSL manage auto-refresh task");
                        }
                    }
                }

                // Start periodic refresh in tokio (off UI thread)
                let task = tokio::spawn(async move {
                    let mut interval = tokio::time::interval(
                        std::time::Duration::from_secs(WSL_MANAGE_REFRESH_INTERVAL_SECS)
                    );
                    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

                    loop {
                        // Refresh live distro state first (wsl -l -v),
                        // then refresh version/status info
                        {
                            let state = as_ptr.lock().await;
                            let _ = state.wsl_dashboard.refresh_distros().await;
                        }
                        crate::ui::handlers::wsl_manage::refresh_wsl_info(&ah, &as_ptr).await;

                        interval.tick().await;
                    }
                });

                // Store handle for later cancellation
                tokio::spawn(async move {
                    let mut handle = refresh_handle.lock().await;
                    *handle = Some(task);
                });
            } else {
                // Left tab 4 — cancel the periodic refresh task
                let refresh_handle = refresh_handle.clone();
                // Cancel immediately without async
                {
                    let handle = refresh_handle.try_lock();
                    if let Ok(mut h) = handle {
                        if let Some(task) = h.take() {
                            task.abort();
                            debug!("Cancelled WSL manage auto-refresh task");
                        }
                    }
                }
            }

            // Tab 6 is "About" — fetch BASE_API once on first visit
            if tab == 6 {
                super::about::trigger_fetch_if_needed(ah.clone(), as_ptr.clone());
            }
        }
    });

    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_save_sidebar_state(move || {
        if let Some(app) = ah.upgrade() {
            let collapsed = app.get_sidebar_collapsed();
            let as_ptr = as_ptr.clone();
            debug!("Saving sidebar state: {}", collapsed);
            tokio::spawn(async move {
                let mut state = as_ptr.lock().await;
                let mut settings = state.config_manager.get_settings().clone();
                if settings.sidebar_collapsed != collapsed {
                    settings.sidebar_collapsed = collapsed;
                    let _ = state.config_manager.update_settings(settings);
                }
            });
        }
    });

    app.on_open_url(move |url| {
        let _ = open::that(url.as_str());
    });

    let ah = app_handle.clone();
    app.on_close_task_status(move || {
        if let Some(app) = ah.upgrade() {
            app.set_task_status_visible(false);
        }
    });
}
