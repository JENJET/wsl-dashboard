use crate::ui::data::refresh_distros_ui;
use crate::ui::handlers::instance;
use crate::{AppState, AppWindow, i18n};
use slint::Model;
use std::os::windows::process::CommandExt;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

pub fn setup(app: &AppWindow, app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    // Handle message link click
    {
        let ah = app_handle.clone();
        app.on_message_link_clicked(move || {
            if let Some(app) = ah.upgrade() {
                let mut link = app.get_current_message_url().to_string();
                if link.is_empty() {
                    link = app.get_current_message_link().to_string();
                }

                if link.starts_with("http://") || link.starts_with("https://") {
                    let _ = open::that(link);
                } else {
                    let path = std::path::Path::new(&link);
                    if path.exists() {
                        let _ = open::that(link);
                    } else if let Ok(startup_dir) = crate::app::autostart::get_startup_dir() {
                        let _ = open::that(startup_dir.to_string_lossy().to_string());
                    }
                }
            }
        });
    }

    // Terminal
    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_terminal_distro(move |name| {
            info!("Operation: Open terminal - {}", name);
            let ah = ah_outer.clone();
            let as_ptr = as_outer.clone();
            tokio::spawn(async move {
                let manager = {
                    let app_state = as_ptr.lock().await;
                    app_state.wsl_dashboard.clone()
                };

                if let Some(op) = manager.get_active_op(&name).await {
                    let msg = i18n::tr("toast.distro_busy", &[name.to_string(), op.to_string()]);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah.upgrade() {
                            app.set_current_message(msg.into());
                            app.set_show_message_dialog(true);
                        }
                    });
                    return;
                }

                {
                    let lock_timeout = std::time::Duration::from_millis(500);
                    if let Ok(app_state) = tokio::time::timeout(lock_timeout, as_ptr.lock()).await {
                        let executor = app_state.wsl_dashboard.executor().clone();
                        let instance_config = app_state.config_manager.get_instance_config(&name);
                        let working_dir = instance_config.terminal_dir.clone();
                        let terminal_proxy_enabled = instance_config.terminal_proxy;
                        let proxy_config =
                            app_state.config_manager.get_network_config().proxy.clone();
                        drop(app_state);

                        let mut proxy_exports: Option<Vec<(String, String)>> = None;
                        if terminal_proxy_enabled
                            && proxy_config.is_enabled
                            && !proxy_config.host.is_empty()
                            && !proxy_config.port.is_empty()
                        {
                            let auth = if proxy_config.auth_enabled
                                && !proxy_config.username.is_empty()
                                && !proxy_config.password.is_empty()
                            {
                                format!("{}:{}@", proxy_config.username, proxy_config.password)
                            } else {
                                "".to_string()
                            };
                            let proxy_url = format!(
                                "http://{}{}:{}",
                                auth, proxy_config.host, proxy_config.port
                            );

                            let mut exports = Vec::new();
                            exports.push(("HTTP_PROXY".to_string(), proxy_url.clone()));
                            exports.push(("HTTPS_PROXY".to_string(), proxy_url.clone()));

                            if !proxy_config.no_proxy.is_empty() {
                                exports
                                    .push(("NO_PROXY".to_string(), proxy_config.no_proxy.clone()));
                            }
                            proxy_exports = Some(exports);
                        }

                        let _ = executor
                            .open_distro_terminal(&name, &working_dir, proxy_exports)
                            .await;
                    }
                }
                refresh_distros_ui(ah, as_ptr).await;
            });
        });
    }

    // Folder
    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_folder_distro(move |name| {
            info!("Operation: Open folder - {}", name);
            let ah = ah_outer.clone();
            let as_ptr = as_outer.clone();
            tokio::spawn(async move {
                let manager = {
                    let app_state = as_ptr.lock().await;
                    app_state.wsl_dashboard.clone()
                };

                if let Some(op) = manager.get_active_op(&name).await {
                    let msg = i18n::tr("toast.distro_busy", &[name.to_string(), op.to_string()]);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah.upgrade() {
                            app.set_current_message(msg.into());
                            app.set_show_message_dialog(true);
                        }
                    });
                    return;
                }

                {
                    let lock_timeout = std::time::Duration::from_millis(500);
                    if let Ok(app_state) = tokio::time::timeout(lock_timeout, as_ptr.lock()).await {
                        let executor = app_state.wsl_dashboard.executor().clone();
                        drop(app_state);
                        let _ = executor.open_distro_folder(&name).await;
                    }
                }
                refresh_distros_ui(ah, as_ptr).await;
            });
        });
    }

    // Open install folder
    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_open_install_folder(move |name| {
            info!("Operation: Open install folder - {}", name);
            let ah = ah_outer.clone();
            let as_ptr = as_outer.clone();
            let distro_name = name.to_string();
            tokio::spawn(async move {
                let manager = {
                    let app_state = as_ptr.lock().await;
                    app_state.wsl_dashboard.clone()
                };
                if let Some(op) = manager.get_active_op(&distro_name).await {
                    let msg = i18n::tr(
                        "toast.distro_busy",
                        &[distro_name.to_string(), op.to_string()],
                    );
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah.upgrade() {
                            app.set_current_message(msg.into());
                            app.set_show_message_dialog(true);
                        }
                    });
                    return;
                }
                let result = crate::wsl::ops::info::get_distro_install_location(
                    &manager.executor(),
                    &distro_name,
                )
                .await;
                if let Some(location) = result.data {
                    if !location.is_empty() {
                        let _ = open::that(&location);
                    }
                }
            });
        });
    }

    // VS Code
    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_vscode_distro(move |name| {
            info!("Operation: Try open VS Code - {}", name);
            let ah = ah_outer.clone();
            let as_ptr = as_outer.clone();
            let _ = slint::spawn_local(async move {
                let manager = {
                    let state = as_ptr.lock().await;
                    state.wsl_dashboard.clone()
                };

                if let Some(op) = manager.get_active_op(&name).await {
                    let msg = i18n::tr("toast.distro_busy", &[name.to_string(), op.to_string()]);
                    if let Some(app) = ah.upgrade() {
                        app.set_current_message(msg.into());
                        app.set_show_message_dialog(true);
                    }
                    return;
                }

                let ah_timer = ah.clone();
                let executor = manager.executor().clone();
                let check_result = crate::wsl::ops::ui::check_vscode_extension(&executor).await;
                let is_valid_version = check_result.success && check_result.output.contains("ms-vscode-remote.remote-wsl");

                if is_valid_version {
                    if let Some(app) = ah.upgrade() {
                        app.set_show_vscode_startup(true);
                    }

                    let working_dir = {
                        let state = as_ptr.lock().await;
                        state.config_manager.get_instance_config(&name).vscode_dir
                    };

                    let _ = executor.open_distro_vscode(&name, &working_dir).await;
                    refresh_distros_ui(ah, as_ptr).await;

                    slint::Timer::single_shot(std::time::Duration::from_secs(6), move || {
                        if let Some(app) = ah_timer.upgrade() {
                            if app.get_show_vscode_startup() {
                                app.set_show_vscode_startup(false);
                            }
                        }
                    });
                } else {
                    let ext_info = {
                        let state = as_ptr.lock().await;
                        state.vscode_extension.clone()
                    };

                    let (ext_name, ext_url) = if let Some(info) = ext_info {
                        (info.name, info.url)
                    } else {
                        ("WSL".to_string(), "https://marketplace.visualstudio.com/items?itemName=ms-vscode-remote.remote-wsl".to_string())
                    };

                    if let Some(app) = ah.upgrade() {
                        app.set_current_message(i18n::t("dialog.vscode_extension_required").into());
                        app.set_current_message_link(ext_name.into());
                        app.set_current_message_url(ext_url.into());
                        app.set_show_message_dialog(true);
                    }
                }
            });
        });
    }

    // Edit .bashrc
    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_edit_bashrc_distro(move |name| {
            info!("Operation: Edit .bashrc - {}", name);
            let ah = ah_outer.clone();
            let as_ptr = as_outer.clone();
            let _ = slint::spawn_local(async move {
                let (dashboard, name_str) = {
                    let app_state = as_ptr.lock().await;
                    (app_state.wsl_dashboard.clone(), name.to_string())
                };

                if let Some(op) = dashboard.get_active_op(&name_str).await {
                    let msg = i18n::tr("toast.distro_busy", &[name_str.clone(), op.to_string()]);
                    if let Some(app) = ah.upgrade() {
                        app.set_current_message(msg.into());
                        app.set_show_message_dialog(true);
                    }
                    return;
                }

                // Sentinel Check: System heavy op?
                if dashboard.heavy_op_lock().try_lock().is_err() {
                    let msg = i18n::t("toast.system_busy");
                    if let Some(app) = ah.upgrade() {
                        app.set_current_message(msg.into());
                        app.set_show_message_dialog(true);
                    }
                    return;
                }

                dashboard.open_distro_bashrc(&name_str).await;
                refresh_distros_ui(ah, as_ptr).await;
            });
        });
    }

    // Information
    {
        static IS_FETCHING_INFO: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        static IS_FETCHING_EXTRA_INFO: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_information_clicked(move |name| {
            if IS_FETCHING_INFO
                .compare_exchange(
                    false,
                    true,
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                )
                .is_err()
            {
                return;
            }

            info!("Operation: Information clicked - {}", name);
            let ah = ah_outer.clone();
            let as_ptr = as_outer.clone();
            let name = name.to_string();
            tokio::spawn(async move {
                let _guard = scopeguard::guard((), |_| {
                    IS_FETCHING_INFO.store(false, std::sync::atomic::Ordering::SeqCst);
                });

                let (dashboard, name_str) = {
                    let app_state = as_ptr.lock().await;
                    (app_state.wsl_dashboard.clone(), name.clone())
                };

                if let Some(op) = dashboard.get_active_op(&name_str).await {
                    let msg = i18n::tr("toast.distro_busy", &[name_str.clone(), op.to_string()]);
                    let ah_clone = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_clone.upgrade() {
                            app.set_current_message(msg.into());
                            app.set_show_message_dialog(true);
                        }
                    });
                    return;
                }

                // Sentinel Check: System heavy op?
                if dashboard.heavy_op_lock().try_lock().is_err() {
                    let msg = i18n::t("toast.system_busy");
                    let ah_clone = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_clone.upgrade() {
                            app.set_current_message(msg.into());
                            app.set_show_message_dialog(true);
                        }
                    });
                    return;
                }

                {
                    let ah_clone = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_clone.upgrade() {
                            app.set_task_status_text(i18n::t("operation.fetching_info").into());
                            app.set_task_status_visible(true);
                        }
                    });
                }
                let result = dashboard.executor().get_distro_information(&name_str).await;
                let ah_ui = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_ui.upgrade() {
                        app.set_task_status_visible(false);
                        if result.success {
                            if let Some(data) = result.data {
                                let is_running = data.status == "Running";
                                let distro_name_for_async = data.distro_name.clone();
                                let vhdx_path_for_async = data.vhdx_path.clone();
                                let mut slint_data = app.get_information();
                                slint_data.distro_name = data.distro_name.into();
                                slint_data.wsl_version = data.wsl_version.into();
                                slint_data.status = data.status.into();
                                slint_data.install_location = data.install_location.into();
                                slint_data.vhdx_path = data.vhdx_path.into();
                                slint_data.vhdx_size = data.vhdx_size.into();
                                slint_data.actual_used = Default::default();
                                slint_data.actual_total = Default::default();
                                slint_data.ip = Default::default();
                                slint_data.vhdx_virtual_size = data.vhdx_virtual_size.into();
                                slint_data.vhdx_type = data.vhdx_type.into();
                                slint_data.vhdx_is_sparse = data.vhdx_is_sparse;
                                app.set_information(slint_data);
                                app.set_show_information(true);

                                if is_running
                                    && IS_FETCHING_EXTRA_INFO
                                        .compare_exchange(
                                            false,
                                            true,
                                            std::sync::atomic::Ordering::SeqCst,
                                            std::sync::atomic::Ordering::SeqCst,
                                        )
                                        .is_ok()
                                {
                                    let ah_async = ah_ui.clone();
                                    tokio::task::spawn_blocking(move || {
                                        let _extra_guard = scopeguard::guard((), |_| {
                                            IS_FETCHING_EXTRA_INFO
                                                .store(false, std::sync::atomic::Ordering::SeqCst);
                                        });
                                        let mut ip_val = String::new();
                                        let mut used_val = String::new();
                                        let mut total_val = String::new();

                                        match crate::network::tracker::get_distro_ip(
                                            &distro_name_for_async,
                                            Some(3),
                                        ) {
                                            Ok(ip) => ip_val = ip,
                                            Err(e) => tracing::debug!(
                                                "Failed to fetch IP for info dialog: {}",
                                                e
                                            ),
                                        }

                                        // Get free space of the VHDX drive partition
                                        let mut drive_free_mb: f64 = 0.0;
                                        if !vhdx_path_for_async.is_empty() {
                                            // Extract drive letter (e.g., "D:\" -> "D:\")
                                            let drive_root = if vhdx_path_for_async.len() >= 3 {
                                                vhdx_path_for_async[..3].to_string()
                                            } else {
                                                vhdx_path_for_async.clone()
                                            };
                                            let free_bytes =
                                                crate::utils::system::get_disk_free_space(
                                                    &drive_root,
                                                );
                                            drive_free_mb = free_bytes as f64 / (1024.0 * 1024.0);
                                        }

                                        let df_output = std::process::Command::new("wsl")
                                            .env("WSL_UTF8", "1")
                                            .args(&[
                                                "-d",
                                                &distro_name_for_async,
                                                "--exec",
                                                "df",
                                                "-B1M",
                                                "/",
                                            ])
                                            .creation_flags(0x08000000)
                                            .output();

                                        if let Ok(out) = df_output {
                                            if out.status.success() {
                                                let stdout =
                                                    crate::wsl::decoder::decode_output(&out.stdout)
                                                        .trim()
                                                        .to_string();
                                                if let Some(second_line) = stdout.lines().nth(1) {
                                                    let parts: Vec<&str> =
                                                        second_line.split_whitespace().collect();
                                                    if parts.len() >= 3 {
                                                        if let Ok(used_mb) = parts[2].parse::<f64>()
                                                        {
                                                            let used_gb = used_mb / 1024.0;
                                                            used_val = format!("{:.2} GB", used_gb);
                                                        }

                                                        let linux_total_mb = if let Ok(total_mb) =
                                                            parts[1].parse::<f64>()
                                                        {
                                                            total_mb
                                                        } else {
                                                            0.0
                                                        };

                                                        let effective_total_mb =
                                                            if drive_free_mb > 0.0 {
                                                                linux_total_mb.min(drive_free_mb)
                                                            } else {
                                                                linux_total_mb
                                                            };

                                                        if effective_total_mb > 0.0 {
                                                            let total_gb =
                                                                effective_total_mb / 1024.0;
                                                            total_val =
                                                                format!("{:.2} GB", total_gb);
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        if !ip_val.is_empty() || !used_val.is_empty() {
                                            let _ = slint::invoke_from_event_loop(move || {
                                                if let Some(app) = ah_async.upgrade() {
                                                    let mut info = app.get_information();
                                                    if !ip_val.is_empty() {
                                                        info.ip = ip_val.into();
                                                    }
                                                    if !used_val.is_empty() {
                                                        info.actual_used = used_val.into();
                                                    }
                                                    if !total_val.is_empty() {
                                                        info.actual_total = total_val.into();
                                                    }
                                                    app.set_information(info);
                                                }
                                            });
                                        }
                                    });
                                }
                            }
                        } else {
                            let err = result.error.unwrap_or_else(|| i18n::t("dialog.error"));
                            app.set_current_message(i18n::tr("dialog.info_failed", &[err]).into());
                            app.set_show_message_dialog(true);
                        }
                    }
                });
            });
        });
    }

    // Settings
    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_settings_clicked(move |name| {
            info!("Operation: Settings clicked - {}", name);
            let ah = ah_outer.clone();
            let as_ptr = as_outer.clone();
            let name = name.to_string();
            let _ = slint::spawn_local(async move {
                let manager = {
                    let app_state = as_ptr.lock().await;
                    app_state.wsl_dashboard.clone()
                };

                if let Some(op) = manager.get_active_op(&name).await {
                    let msg = i18n::tr("toast.distro_busy", &[name.clone(), op.to_string()]);
                    if let Some(app) = ah.upgrade() {
                        app.set_current_message(msg.into());
                        app.set_show_message_dialog(true);
                    }
                    return;
                }

                if let Some(app) = ah.upgrade() {
                    let mut is_default = false;
                    {
                        let distros = app.get_distros();
                        for i in 0..distros.row_count() {
                            if let Some(d) = distros.row_data(i) {
                                if d.name == name {
                                    is_default = d.is_default;
                                    break;
                                }
                            }
                        }
                    }

                    let instance_config = {
                        let state = as_ptr.lock().await;
                        state.config_manager.get_instance_config(&name)
                    };

                    app.set_settings_distro_name(name.clone().into());
                    app.set_settings_is_default(is_default);
                    app.set_settings_lock_default(is_default);
                    app.set_settings_terminal_dir(instance_config.terminal_dir.into());
                    app.set_settings_vscode_dir(instance_config.vscode_dir.into());
                    app.set_settings_startup_script(instance_config.startup_script.into());
                    app.set_settings_terminal_proxy(instance_config.terminal_proxy);
                    let is_task_exists = crate::network::scheduler::check_task_exists();
                    app.set_settings_autostart(instance_config.auto_startup && is_task_exists);
                    app.set_settings_is_task_exists(is_task_exists);
                    app.set_settings_terminal_dir_error("".into());
                    app.set_settings_vscode_dir_error("".into());
                    app.set_settings_startup_script_error("".into());
                    app.set_settings_default_error("".into());
                    app.set_show_settings(true);

                    let ah_fetch = ah.clone();
                    let as_fetch = as_ptr.clone();
                    tokio::spawn(async move {
                        instance::fetch_latest_instance_data(ah_fetch, as_fetch).await;
                    });
                }
            });
        });
    }

    // Settings confirm
    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_confirm_distro_settings(
            move |name,
                  terminal_dir,
                  vscode_dir,
                  is_default,
                  autostart,
                  startup_script,
                  terminal_proxy| {
                let ah = ah_outer.clone();
                let as_ptr = as_outer.clone();
                let name = name.to_string();
                let terminal_dir = terminal_dir.to_string();
                let vscode_dir = vscode_dir.to_string();
                let startup_script = startup_script.to_string();

                let _ = slint::spawn_local(async move {
                    super::settings_logic::perform_save_settings(
                        ah,
                        as_ptr,
                        name,
                        terminal_dir,
                        vscode_dir,
                        is_default,
                        autostart,
                        startup_script,
                        terminal_proxy,
                    )
                    .await;
                });
            },
        );
    }

    // WSL Config click
    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_configs_clicked(move |name| {
            let ah = ah_outer.clone();
            let as_ptr = as_outer.clone();
            let name = name.to_string();
            tokio::spawn(async move {
                let manager = {
                    let app_state = as_ptr.lock().await;
                    app_state.wsl_dashboard.clone()
                };

                if let Some(op) = manager.get_active_op(&name).await {
                    let msg = i18n::tr("toast.distro_busy", &[name.clone(), op.to_string()]);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah.upgrade() {
                            app.set_current_message(msg.into());
                            app.set_show_message_dialog(true);
                        }
                    });
                    return;
                }

                // Sentinel Check: System heavy op?
                if manager.heavy_op_lock().try_lock().is_err() {
                    let msg = i18n::t("toast.system_busy");
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah.upgrade() {
                            app.set_current_message(msg.into());
                            app.set_show_message_dialog(true);
                        }
                    });
                    return;
                }

                super::config_logic::handle_configs_clicked(ah, as_ptr, name.to_string()).await;
            });
        });
    }

    // Config Preview
    {
        let ah = app_handle.clone();
        app.on_request_wsl_config_preview(move || {
            let ah = ah.clone();
            let _ = slint::spawn_local(async move {
                super::config_logic::handle_request_preview(ah).await;
            });
        });
    }

    // Config Save
    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_save_wsl_config(move || {
            let ah = ah_outer.clone();
            let as_ptr = as_outer.clone();
            tokio::spawn(async move {
                super::config_logic::handle_save_wsl_config(ah, as_ptr, false).await;
            });
        });
    }

    // Config Save & Restart
    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_save_wsl_config_and_restart(move || {
            let ah = ah_outer.clone();
            let as_ptr = as_outer.clone();
            tokio::spawn(async move {
                super::config_logic::handle_save_wsl_config(ah, as_ptr, true).await;
            });
        });
    }

    // Home click
    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_home_clicked(move || {
            let as_ptr = as_outer.clone();
            if let Some(app) = ah_outer.upgrade() {
                let is_visible = app.get_is_window_visible();
                tokio::spawn(async move {
                    if crate::ui::data::should_refresh_wsl("manual trigger", is_visible) {
                        let dashboard = {
                            let state = as_ptr.lock().await;
                            state.wsl_dashboard.clone()
                        };
                        let _ = dashboard.refresh_distros().await;
                    }
                });
            }
        });
    }

    // Set default distro
    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_set_default_distro(move |name| {
            info!("Operation: Set default distro - {}", name);
            let ah = ah_outer.clone();
            let as_ptr = as_outer.clone();
            tokio::spawn(async move {
                let manager = {
                    let app_state = as_ptr.lock().await;
                    app_state.wsl_dashboard.clone()
                };

                if let Some(op) = manager.get_active_op(&name).await {
                    let msg = i18n::tr("toast.distro_busy", &[name.to_string(), op.to_string()]);
                    let ah_clone = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_clone.upgrade() {
                            app.set_current_message(msg.into());
                            app.set_show_message_dialog(true);
                        }
                    });
                    return;
                }

                // Execute the command to set default
                let executor = manager.executor().clone();
                let result = executor.execute_command(&["--set-default", &name]).await;

                if result.success {
                    let msg = i18n::tr("toast.default_set_success", &[name.to_string()]);
                    let ah_clone = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_clone.upgrade() {
                            app.set_current_message(msg.into());
                            app.set_show_message_dialog(true);
                        }
                    });

                    // Refresh distros UI immediately after successful operation
                    refresh_distros_ui(ah.clone(), as_ptr.clone()).await;
                } else {
                    let error_msg = result.error.unwrap_or_else(|| "Unknown error".to_string());
                    let msg = i18n::tr("toast.default_set_failed", &[name.to_string(), error_msg]);
                    let ah_clone = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_clone.upgrade() {
                            app.set_current_message(msg.into());
                            app.set_show_message_dialog(true);
                        }
                    });

                    // Also refresh UI even on failure to ensure consistency
                    refresh_distros_ui(ah.clone(), as_ptr.clone()).await;
                }
            });
        });
    }

    // Copy to clipboard
    {
        app.on_copy_to_clipboard(move |text| {
            use crate::utils::system::copy_to_clipboard;
            let text_str = text.to_string();
            match copy_to_clipboard(&text_str) {
                Ok(_) => {
                    tracing::info!("Copied to clipboard: {}", text_str);
                }
                Err(e) => {
                    tracing::error!("Failed to copy to clipboard: {}", e);
                }
            }
        });
    }

    // Show copy success toast
    {
        let ah = app_handle.clone();
        app.on_show_copy_success(move || {
            if let Some(app) = ah.upgrade() {
                let msg = i18n::t("toast.copy_success");
                app.set_task_status_text(msg.into());
                app.set_task_status_visible(true);

                // Auto-hide toast after 3 seconds
                let ah_inner = ah.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_inner.upgrade() {
                            app.set_task_status_visible(false);
                        }
                    });
                });
            }
        });
    }

    // Copy IP to clipboard
    {
        let ah = app_handle.clone();
        app.on_copy_ip_clicked(move || {
            if let Some(app) = ah.upgrade() {
                let info = app.get_information();
                let ip = info.ip.to_string();
                if !ip.is_empty() {
                    use crate::utils::system::copy_to_clipboard;
                    match copy_to_clipboard(&ip) {
                        Ok(_) => {
                            tracing::info!("Copied IP to clipboard: {}", ip);
                            let ah_inner = ah.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(app) = ah_inner.upgrade() {
                                    app.invoke_show_copy_success();
                                }
                            });
                        }
                        Err(e) => {
                            tracing::error!("Failed to copy IP to clipboard: {}", e);
                        }
                    }
                }
            }
        });
    }

    // Copy distro IP to clipboard (from distro card)
    {
        let ah = app_handle.clone();
        app.on_copy_distro_ip(move |ip| {
            let ip_str = ip.to_string();
            if ip_str.is_empty() {
                return;
            }
            use crate::utils::system::copy_to_clipboard;
            match copy_to_clipboard(&ip_str) {
                Ok(_) => {
                    tracing::info!("Copied distro IP to clipboard: {}", ip_str);
                    let ah_inner = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_inner.upgrade() {
                            app.invoke_show_copy_success();
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("Failed to copy distro IP to clipboard: {}", e);
                }
            }
        });
    }

    // VHDX Resize - show dialog
    {
        let ah = app_handle.clone();
        app.on_vhdx_resize_show(move |distro_name, vhdx_path| {
            if let Some(app) = ah.upgrade() {
                app.set_vhdx_resize_distro_name(distro_name);
                app.set_vhdx_resize_path(vhdx_path);
                app.set_vhdx_resize_new_size("".into());
                app.set_vhdx_resize_error("".into());
                app.set_vhdx_resize_output("".into());
                app.set_vhdx_resize_is_error(false);
                app.set_vhdx_resize_running(false);
                app.set_show_vhdx_resize(true);
            }
        });
    }

    // VHDX Resize - cancel
    {
        let ah = app_handle.clone();
        app.on_vhdx_resize_cancel(move || {
            if let Some(app) = ah.upgrade() {
                app.set_show_vhdx_resize(false);
                app.set_vhdx_resize_error("".into());
            }
        });
    }

    // VHDX Resize - confirm
    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_vhdx_resize_confirm(move |new_size_gb| {
            let size_str = new_size_gb.to_string();
            tracing::info!("VHDX resize confirm clicked, size: '{}'", size_str);

            // Read UI properties NOW (on Slint event loop thread) before spawning
            let (vhdx_path, distro_name) = {
                if let Some(app) = ah_outer.upgrade() {
                    (
                        app.get_vhdx_resize_path().to_string(),
                        app.get_vhdx_resize_distro_name().to_string(),
                    )
                } else {
                    tracing::error!("VHDX resize: failed to get app handle from UI thread");
                    return;
                }
            };

            tracing::info!("VHDX resize: path={}, distro={}", vhdx_path, distro_name);

            let ah = ah_outer.clone();
            let as_ptr = as_outer.clone();

            // Show running state and output area
            {
                if let Some(app) = ah.upgrade() {
                    app.set_vhdx_resize_running(true);
                    app.set_vhdx_resize_output("".into());
                }
            }

            tokio::spawn(async move {
                // Parse and validate size
                let size_gb: f64 = match size_str.trim().parse() {
                    Ok(v) if v > 0.0 => v,
                    _ => {
                        let ah2 = ah.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah2.upgrade() {
                                app.set_vhdx_resize_output(
                                    i18n::t("dialog.vhdx_resize_failed").into(),
                                );
                                app.set_vhdx_resize_running(false);
                            }
                        });
                        return;
                    }
                };

                // Check if distro is running
                let is_running = {
                    let state = as_ptr.lock().await;
                    let distros = state.wsl_dashboard.get_distros().await;
                    distros.iter().any(|d| {
                        d.name == distro_name
                            && matches!(d.status, crate::wsl::models::WslStatus::Running)
                    })
                };

                if is_running {
                    let ah2 = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah2.upgrade() {
                            app.set_vhdx_resize_output(
                                i18n::t("dialog.vhdx_resize_running").into(),
                            );
                            app.set_vhdx_resize_running(false);
                        }
                    });
                    return;
                }

                let new_size_bytes = (size_gb * 1024.0 * 1024.0 * 1024.0) as u64;

                // Update output with progress
                {
                    let ah2 = ah.clone();
                    let msg = i18n::tr("dialog.vhdx_resize_progress", &[format!("{:.0}", size_gb)]);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah2.upgrade() {
                            app.set_vhdx_resize_output(msg.into());
                        }
                    });
                }

                tracing::info!(
                    "VHDX resize: calling resize_vhdx with {} bytes",
                    new_size_bytes
                );
                let result = tokio::task::spawn_blocking(move || {
                    crate::wsl::ops::vhdx::resize_vhdx(&vhdx_path, new_size_bytes)
                })
                .await;

                // Helper to stop running after a delay so the auto-scroll Timer can fire first
                let stop_running = |ah: slint::Weak<AppWindow>| {
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah.upgrade() {
                                app.set_vhdx_resize_running(false);
                            }
                        });
                    });
                };

                match result {
                    Ok(Ok(())) => {
                        let ah2 = ah.clone();
                        let ah_stop = ah.clone();
                        let msg =
                            i18n::tr("dialog.vhdx_resize_success", &[format!("{:.0}", size_gb)]);
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah2.upgrade() {
                                app.set_vhdx_resize_output(msg.into());
                            }
                        });
                        stop_running(ah_stop);
                    }
                    Ok(Err(e)) => {
                        let ah2 = ah.clone();
                        let ah_stop = ah.clone();
                        let msg = i18n::tr("dialog.vhdx_resize_failed", &[e]);
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah2.upgrade() {
                                app.set_vhdx_resize_is_error(true);
                                app.set_vhdx_resize_output(msg.into());
                            }
                        });
                        stop_running(ah_stop);
                    }
                    Err(e) => {
                        let ah2 = ah.clone();
                        let ah_stop = ah.clone();
                        let msg = i18n::tr("dialog.vhdx_resize_failed", &[e.to_string()]);
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah2.upgrade() {
                                app.set_vhdx_resize_is_error(true);
                                app.set_vhdx_resize_output(msg.into());
                            }
                        });
                        stop_running(ah_stop);
                    }
                }
            });
        });
    }

    // Set sparse - cancel
    {
        let ah = app_handle.clone();
        app.on_cancel_set_sparse(move || {
            if let Some(app) = ah.upgrade() {
                app.set_show_set_sparse_confirm(false);
            }
        });
    }

    // Set sparse - confirm
    {
        let ah_outer = app_handle.clone();
        app.on_confirm_set_sparse(move || {
            let ah_close = ah_outer.clone();
            // Close confirmation dialog immediately
            if let Some(app) = ah_close.upgrade() {
                app.set_show_set_sparse_confirm(false);
            }
            let ah = ah_outer.clone();
            let vhdx_path = {
                if let Some(app) = ah.upgrade() {
                    app.get_information().vhdx_path.to_string()
                } else {
                    return;
                }
            };

            tokio::spawn(async move {
                let ah2 = ah.clone();
                let result = tokio::task::spawn_blocking(move || {
                    crate::wsl::ops::vhdx::set_sparse_file(&vhdx_path)
                })
                .await;

                match result {
                    Ok(Ok(())) => {
                        let msg = i18n::tr("dialog.vhdx_set_sparse_success", &[]);
                        // Update information to reflect sparse status
                        let ah3 = ah2.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah3.upgrade() {
                                let mut info = app.get_information();
                                info.vhdx_is_sparse = true;
                                app.set_information(info);
                                app.set_current_message(msg.into());
                                app.set_show_message_dialog(true);
                            }
                        });
                    }
                    Ok(Err(e)) => {
                        let msg = i18n::tr("dialog.vhdx_set_sparse_failed", &[e]);
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah2.upgrade() {
                                app.set_current_message(msg.into());
                                app.set_show_message_dialog(true);
                            }
                        });
                    }
                    Err(e) => {
                        let msg = i18n::tr("dialog.vhdx_set_sparse_failed", &[e.to_string()]);
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah2.upgrade() {
                                app.set_current_message(msg.into());
                                app.set_show_message_dialog(true);
                            }
                        });
                    }
                }
            });
        });
    }
}
