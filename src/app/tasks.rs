use crate::config::models::CachedDistro;
use crate::ui::data::refresh_distros_ui;
use crate::wsl::models::WslStatus;
use crate::{AppState, AppWindow};
use slint::{ComponentHandle, Model};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

// Combined WSL state + resource monitor.
// Runs every 5s on the Home tab: refreshes distro list (wsl -l -v) and fetches
// CPU/IP resource data for running distros.  Replaces the former two separate
// 5s timers (spawn_wsl_monitor + spawn_resource_monitor) that ran independently
// and duplicated work.
pub fn spawn_state_monitor(app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;

            let ah = app_handle.clone();
            let as_ptr = app_state.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah.upgrade() {
                    // Only refresh if the Home tab (index 0) is selected
                    if app.get_selected_tab() != 0 {
                        return;
                    }
                    let is_visible = app.get_is_window_visible();
                    let is_batch_operating = app.get_batch_operating();
                    let as_ptr_res = as_ptr.clone();
                    let ah_weak = app.as_weak();
                    tokio::spawn(async move {
                        let dashboard = {
                            let state = as_ptr_res.lock().await;
                            state.wsl_dashboard.clone()
                        };

                        // Phase 1: Refresh WSL distro list (triggers state_listener → refresh_distros_ui)
                        let _ = dashboard.refresh_distros().await;

                        // Phase 2: Fetch CPU/IP for running distros (only when window is visible)
                        if !is_visible {
                            return;
                        }
                        if is_batch_operating
                            || !crate::ui::data::should_refresh_wsl("periodic trigger", is_visible)
                        {
                            return;
                        }
                        let running = dashboard.get_distros().await;
                        let running_names: Vec<String> = running
                            .into_iter()
                            .filter(|d| matches!(d.status, WslStatus::Running))
                            .map(|d| d.name)
                            .collect();
                        if running_names.is_empty() {
                            return;
                        }

                        tokio::task::spawn_blocking(move || {
                            let show_ip = crate::utils::wsl_config::show_distro_ip();
                            let mut ip_results: Vec<(String, String)> = Vec::new();
                            let mut resource_results: Vec<(String, f64, f64, f64)> = Vec::new();

                            for name in &running_names {
                                if show_ip {
                                    if let Ok(ip) =
                                        crate::network::tracker::get_distro_ip(name, Some(1))
                                    {
                                        ip_results.push((name.clone(), ip));
                                    }
                                }
                                if let Ok((cpu, mem_used, mem_total)) =
                                    crate::network::tracker::get_distro_resource_usage(name)
                                {
                                    resource_results.push((name.clone(), cpu, mem_used, mem_total));
                                }
                            }

                            if ip_results.is_empty() && resource_results.is_empty() {
                                return;
                            }

                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(app) = ah_weak.upgrade() {
                                    let model = app.get_distros();
                                    for i in 0..model.row_count() {
                                        if let Some(mut distro) = model.row_data(i) {
                                            let mut updated = false;

                                            if let Some((_, ip)) =
                                                ip_results.iter().find(|(n, _)| distro.name == *n)
                                            {
                                                if distro.ip != ip.as_str() {
                                                    distro.ip = ip.into();
                                                    updated = true;
                                                }
                                            }

                                            if let Some((_, cpu, mem_used, mem_total)) =
                                                resource_results
                                                    .iter()
                                                    .find(|(n, _, _, _)| distro.name == *n)
                                            {
                                                let cpu_str = format!("{:.1}%", cpu);
                                                let mem_str =
                                                    format!("{:.1}/{:.1} GB", mem_used, mem_total);

                                                if distro.cpu_usage != cpu_str.as_str() {
                                                    distro.cpu_usage = cpu_str.into();
                                                    updated = true;
                                                }
                                                if distro.memory_usage != mem_str.as_str() {
                                                    distro.memory_usage = mem_str.into();
                                                    updated = true;
                                                }
                                            } else if distro.status != "Running" {
                                                if !distro.cpu_usage.is_empty() {
                                                    distro.cpu_usage = Default::default();
                                                    updated = true;
                                                }
                                                if !distro.memory_usage.is_empty() {
                                                    distro.memory_usage = Default::default();
                                                    updated = true;
                                                }
                                            }

                                            if updated {
                                                model.set_row_data(i, distro);
                                            }
                                        }
                                    }
                                }
                            });
                        });
                    });
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
