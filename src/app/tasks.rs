use crate::config::models::CachedDistro;
use crate::ui::data::refresh_distros_ui;
use crate::wsl::models::{WslStatus, WslVersion};
use crate::{AppState, AppWindow};
use slint::{ComponentHandle, Model};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

const POLL_INTERVAL_SECS: u64 = 5;

// Adaptive WSL state + resource monitor.
// Polls distro list (wsl -l -v) and fetches CPU/IP for running distros.
// Skips entirely when not on Home tab; reduces to 30s when window is hidden.
pub fn spawn_state_monitor(app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    tokio::spawn(async move {
        loop {
            // Skip entirely if not on Home tab — avoids unnecessary event loop wakeups
            if crate::ui::data::CURRENT_TAB.load(std::sync::atomic::Ordering::Relaxed) != 0 {
                tokio::time::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;
                continue;
            }

            // Visible → 5s interval; hidden → sleep until window restored (REFRESH_NOTIFY)
            let visible =
                crate::ui::data::WINDOW_VISIBLE.load(std::sync::atomic::Ordering::Relaxed);
            if visible {
                tokio::time::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;
            } else {
                crate::ui::data::REFRESH_NOTIFY.notified().await;
            }

            // Re-check after sleep in case state changed during sleep
            if crate::ui::data::CURRENT_TAB.load(std::sync::atomic::Ordering::Relaxed) != 0 {
                continue;
            }

            let ah = app_handle.clone();
            let as_ptr = app_state.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah.upgrade() {
                    // Double-check tab inside event loop (race condition safety)
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
                        type DistroInfo = (String, WslVersion);
                        let running_info: Vec<DistroInfo> = running
                            .into_iter()
                            .filter(|d| matches!(d.status, WslStatus::Running))
                            .filter(|d| !dashboard.has_stopping_op(&d.name))
                            .map(|d| (d.name, d.version))
                            .collect();
                        if running_info.is_empty() {
                            return;
                        }

                        tokio::task::spawn_blocking(move || {
                            let show_ip = crate::utils::wsl_config::show_distro_ip();
                            let mut ip_results: Vec<(String, String)> = Vec::new();
                            let mut resource_results: Vec<(String, f64, f64)> = Vec::new();

                            for (name, _) in &running_info {
                                if show_ip {
                                    if let Ok(ip) =
                                        crate::network::tracker::get_distro_ip(name, Some(1))
                                    {
                                        ip_results.push((name.clone(), ip));
                                    }
                                }
                                let (cpu, mem) =
                                    crate::network::tracker::get_distro_resource_usage(name);
                                resource_results.push((name.clone(), cpu, mem));
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

                                            if let Some((_, cpu, mem)) = resource_results
                                                .iter()
                                                .find(|(n, _, _)| distro.name == *n)
                                            {
                                                let cpu_str = format!("{:.2}%", cpu);
                                                if distro.cpu_usage != cpu_str.as_str() {
                                                    distro.cpu_usage = cpu_str.into();
                                                    updated = true;
                                                }
                                                let mem_kib = *mem;
                                                let mem_str = if mem_kib >= 1024.0 * 1024.0 {
                                                    format!("{:.2} GB", mem_kib / (1024.0 * 1024.0))
                                                } else {
                                                    format!("{:.2} MB", mem_kib / 1024.0)
                                                };
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

// Adaptive USB status monitoring task — only wakes on USB tab
pub fn spawn_usb_monitor(app_handle: slint::Weak<AppWindow>) {
    tokio::spawn(async move {
        loop {
            // Skip entirely if not on USB tab
            if crate::ui::data::CURRENT_TAB.load(std::sync::atomic::Ordering::Relaxed) != 2 {
                tokio::time::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;
                continue;
            }

            // Visible → 5s interval; hidden → sleep until window restored
            let visible =
                crate::ui::data::WINDOW_VISIBLE.load(std::sync::atomic::Ordering::Relaxed);
            if visible {
                tokio::time::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;
            } else {
                crate::ui::data::REFRESH_NOTIFY.notified().await;
            }

            // Re-check after sleep
            if crate::ui::data::CURRENT_TAB.load(std::sync::atomic::Ordering::Relaxed) != 2 {
                continue;
            }

            let ah = app_handle.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah.upgrade() {
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
