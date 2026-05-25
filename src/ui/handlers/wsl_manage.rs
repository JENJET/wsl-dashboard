use crate::i18n;
use crate::ui::data;
use crate::utils::system::CREATE_NO_WINDOW;
use crate::{AppState, AppWindow, WslManageStrings};
use std::os::windows::process::CommandExt;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info};

pub fn setup(app: &AppWindow, app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    // Refresh WSL info when the tab is first opened
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_manage_refresh(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let _ = slint::spawn_local(async move {
            refresh_wsl_info(&ah, &as_ptr).await;
        });
    });

    // Install WSL
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_manage_install(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let _ = slint::spawn_local(async move {
            let executor = {
                let state = as_ptr.lock().await;
                state.wsl_dashboard.executor().clone()
            };
            match executor.spawn_console(&["--install"]).await {
                Ok(()) => {
                    info!("WSL install command launched");
                    let msg = i18n::t("wsl_manage.install_success");
                    data::show_message(&ah, &msg);
                }
                Err(e) => {
                    error!("Failed to launch WSL install: {}", e);
                    let msg = i18n::tr("wsl_manage.install_failed", &[e]);
                    data::show_message(&ah, &msg);
                }
            }
        });
    });

    // Start WSL (open default distro terminal with configured terminal emulator)
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_manage_start(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let _ = slint::spawn_local(async move {
            let default_distro_name = {
                let state = as_ptr.lock().await;
                state
                    .wsl_dashboard
                    .get_distros()
                    .await
                    .iter()
                    .find(|d| d.is_default)
                    .map(|d| d.name.clone())
                    .unwrap_or_default()
            };
            if default_distro_name.is_empty() {
                error!("No default WSL distro found");
                return;
            }
            crate::ui::handlers::resolve_and_open_terminal(&as_ptr, &default_distro_name, &ah)
                .await;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            refresh_wsl_info(&ah, &as_ptr).await;
        });
    });

    // Update WSL
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_manage_update(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        tokio::spawn(async move {
            let executor = {
                let state = as_ptr.lock().await;
                state.wsl_dashboard.executor().clone()
            };

            // Step 1: Get current WSL version
            let version_result = executor.execute_command(&["--version"]).await;
            if !version_result.success {
                let msg = i18n::t("wsl_manage.update_check_failed");
                data::show_message(&ah, &msg);
                return;
            }

            // Parse current version: extract from "WSL 版本: 2.7.3.0" or "WSL version: 2.7.3.0"
            let raw_version = version_result
                .output
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();

            let current_version_num = extract_version_number(&raw_version);

            // Step 2: Show confirmation dialog
            let has_running = {
                let state = as_ptr.lock().await;
                let distros = state.wsl_dashboard.get_distros().await;
                distros
                    .iter()
                    .any(|d| matches!(d.status, crate::wsl::models::WslStatus::Running))
            };

            let confirm_message = if has_running {
                i18n::tr(
                    "wsl_manage.update_confirm_with_running",
                    &[current_version_num],
                )
            } else {
                i18n::tr(
                    "wsl_manage.update_confirm_no_running",
                    &[current_version_num],
                )
            };

            let ah2 = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah2.upgrade() {
                    app.set_wsl_update_confirm_message(confirm_message.into());
                    app.set_show_wsl_update_confirm(true);
                }
            });
        });
    });

    // WSL Update Confirmed (user clicked OK on confirmation dialog)
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_manage_update_confirmed(move |preview| {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        tokio::spawn(async move {
            let ah_ui = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_ui.upgrade() {
                    app.set_wsl_is_updating(true);
                }
            });

            let executor = {
                let state = as_ptr.lock().await;
                state.wsl_dashboard.executor().clone()
            };

            let args = if preview {
                vec!["--update", "--pre-release"]
            } else {
                vec!["--update"]
            };
            let result = executor.execute_command(&args).await;

            let ah_ui = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_ui.upgrade() {
                    app.set_wsl_is_updating(false);
                }
            });

            if result.success {
                info!("WSL update completed successfully");
                let output_lower = result.output.to_lowercase();
                if output_lower.contains("no update") || output_lower.contains("already") {
                    let msg = i18n::t("wsl_manage.update_already_latest");
                    data::show_message(&ah, &msg);
                } else {
                    let msg = i18n::t("wsl_manage.update_success");
                    data::show_message(&ah, &msg);
                    refresh_wsl_info(&ah, &as_ptr).await;
                }
            } else {
                error!(
                    "WSL update failed: {}",
                    result.error.as_deref().unwrap_or("unknown error")
                );
                let msg = i18n::tr(
                    "wsl_manage.update_failed",
                    &[result.error.unwrap_or_else(|| "unknown error".to_string())],
                );
                data::show_message(&ah, &msg);
            }
        });
    });

    // Shutdown all WSL instances
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_manage_shutdown_all(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let _ = slint::spawn_local(async move {
            let result = {
                let state = as_ptr.lock().await;
                state.wsl_dashboard.shutdown_wsl().await
            };

            if result.success {
                info!("WSL shutdown all completed");
                // Refresh state first so the UI shows updated status
                let state = as_ptr.lock().await;
                let _ = state.wsl_dashboard.refresh_distros().await;
                drop(state);
                refresh_wsl_info(&ah, &as_ptr).await;
                // Show success dialog after everything is settled
                let msg = i18n::t("wsl_manage.shutdown_success");
                data::show_message(&ah, &msg);
            } else {
                error!(
                    "WSL shutdown failed: {}",
                    result.error.as_deref().unwrap_or("unknown error")
                );
                let msg = i18n::tr(
                    "wsl_manage.shutdown_failed",
                    &[result.error.unwrap_or_else(|| "unknown error".to_string())],
                );
                data::show_message(&ah, &msg);
            }
        });
    });

    // Set default WSL version
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_manage_set_default_version(move |version| {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let _ = slint::spawn_local(async move {
            let executor = {
                let state = as_ptr.lock().await;
                state.wsl_dashboard.executor().clone()
            };
            let result = executor
                .execute_command(&["--set-default-version", &version.to_string()])
                .await;
            if result.success {
                info!("WSL default version set to {}", version);
                if let Some(app) = ah.upgrade() {
                    app.set_wsl_default_version(version);
                }
                let msg = i18n::t("wsl_manage.set_default_success");
                data::show_message(&ah, &msg);
                refresh_wsl_info(&ah, &as_ptr).await;
            } else {
                let err = result.error.unwrap_or_default();
                error!("Failed to set WSL default version: {}", err);
                let msg = i18n::tr("wsl_manage.set_default_failed", &[err]);
                data::show_message(&ah, &msg);
            }
        });
    });

    // Uninstall WSL
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_manage_uninstall(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let _ = slint::spawn_local(async move {
            let executor = {
                let state = as_ptr.lock().await;
                state.wsl_dashboard.executor().clone()
            };
            match executor.spawn_console(&["--uninstall"]).await {
                Ok(()) => {
                    info!("WSL uninstall command launched");
                    let msg = i18n::t("wsl_manage.uninstall_success");
                    data::show_message(&ah, &msg);
                }
                Err(e) => {
                    error!("Failed to launch WSL uninstall: {}", e);
                    let msg = i18n::tr("wsl_manage.uninstall_failed", &[e]);
                    data::show_message(&ah, &msg);
                }
            }
        });
    });

    // Auto-save VHDX sparse mode when toggled in UI
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_vhdx_sparse_mode_toggled(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let _ = slint::spawn_local(async move {
            if let Some(app) = ah.upgrade() {
                let sparse_mode = app.get_vhdx_sparse_mode();
                let mut state = as_ptr.lock().await;
                let mut settings = state.config_manager.get_settings().clone();
                settings.vhdx_sparse_mode = sparse_mode;
                if let Err(e) = state.config_manager.update_settings(settings) {
                    tracing::error!("Failed to save VHDX sparse mode setting: {}", e);
                } else {
                    tracing::info!("VHDX sparse mode setting saved: {}", sparse_mode);
                }
            }
        });
    });

    // Open WSL Settings (directly launch wslsettings.exe)
}

/// Fetch WSL version and status info and update the UI
pub async fn refresh_wsl_info(
    app_handle: &slint::Weak<AppWindow>,
    app_state: &Arc<Mutex<AppState>>,
) {
    let executor = {
        let state = app_state.lock().await;
        state.wsl_dashboard.executor().clone()
    };

    // Get WSL version info
    let version_result = executor.execute_command(&["--version"]).await;
    let status_result = executor.execute_command(&["--status"]).await;

    // Check if any distros are running and find default distro name
    let (has_running, has_instances, default_distro_name) = {
        let state = app_state.lock().await;
        let distros = state.wsl_dashboard.get_distros().await;
        let has_running = distros
            .iter()
            .any(|d| matches!(d.status, crate::wsl::models::WslStatus::Running));
        let has_instances = distros.len() > 0;
        let default_name = distros
            .iter()
            .find(|d| d.is_default)
            .map(|d| d.name.clone())
            .unwrap_or_default();
        (has_running, has_instances, default_name)
    };

    // Fetch WSL process resource usage (async, off UI thread)
    let resource_lines: Vec<slint::SharedString> = tokio::task::spawn_blocking(|| {
            let mut lines: Vec<slint::SharedString> = Vec::new();
            if let Ok(ps) = std::process::Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-Command",
                    "$p=Get-Process vmmem*,wsl,wslhost -ErrorAction SilentlyContinue;if($p){$mem=[math]::Round(($p|Measure-Object -Property WorkingSet64 -Sum).Sum/1MB,1);try{$s=Get-Counter '\\Process(vmmem*)\\% Processor Time','\\Process(wsl)\\% Processor Time','\\Process(wslhost)\\% Processor Time' -ErrorAction Stop;$t=($s.CounterSamples|Measure-Object -Property CookedValue -Sum).Sum;$c=(Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors;$cpu=[math]::Round($t/$c,1)}catch{$cpu=0};Write-Output \"$cpu|$mem\"}",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
            {
                let out = String::from_utf8_lossy(&ps.stdout).trim().to_string();
                if let Some((cpu, mem)) = out.split_once('|') {
                    if let Ok(mem_mb) = mem.trim().parse::<f64>() {
                        if mem_mb > 0.0 {
                            lines.push("\n---------------------\n".into());
                            if let Ok(cpu_val) = cpu.trim().parse::<f64>() {
                                let cpu_label = i18n::t("distro.cpu_tooltip");
                                lines.push(format!("{}: {:.1}%", cpu_label, cpu_val).into());
                            }
                            let mem_label = i18n::t("distro.memory_tooltip");
                            if mem_mb >= 1024.0 {
                                lines.push(format!("{}: {:.1} GB", mem_label, mem_mb / 1024.0).into());
                            } else {
                                lines.push(format!("{}: {:.0} MB", mem_label, mem_mb).into());
                            }
                        }
                    }
                }
            }
            lines
    })
    .await
    .unwrap_or_default();

    let ah = app_handle.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = ah.upgrade() {
            if version_result.success {
                app.set_wsl_installed(true);

                // Pass raw output to the UI (joined as single text)
                let version_text: slint::SharedString = version_result
                    .output
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join("\n")
                    .into();
                app.set_wsl_version_output(version_text);

                // Still parse default_version from status for the toggle buttons
            } else {
                app.set_wsl_installed(false);
                app.set_wsl_version_output("".into());
                debug!("WSL --version failed, WSL may not be installed or is inbox version");
            }

            if status_result.success {
                let mut status_parts: Vec<&str> = status_result
                    .output
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .collect();

                if !resource_lines.is_empty() {
                    status_parts.extend(resource_lines.iter().map(|s| s.as_str()));
                }

                let status_text: slint::SharedString = status_parts.join("\n").into();
                app.set_wsl_status_output(status_text);

                // Parse default version from status output
                let output = status_result.output.clone();
                for line in output.lines() {
                    let line = line.trim();
                    if let Some((key, value)) = line.split_once(':') {
                        let key_lower = key.to_lowercase().trim().to_string();
                        let val = value.trim().to_string();
                        if key_lower.contains("default version") || key_lower.contains("默认版本")
                        {
                            if let Ok(v) = val.parse::<i32>() {
                                app.set_wsl_default_version(v);
                            } else if val.contains(crate::wsl::models::WslVersion::V2.as_string()) {
                                app.set_wsl_default_version(2);
                            } else {
                                app.set_wsl_default_version(1);
                            }
                        }
                    }
                }
            } else {
                app.set_wsl_status_output("".into());
            }

            app.set_wsl_has_running(has_running);
            app.set_wsl_has_instances(has_instances);
            app.set_wsl_default_distro_name(default_distro_name.into());
        }
    });
}

/// Load WSL manage strings into the UI (called during i18n refresh)
pub fn load_wsl_manage_strings(app: &AppWindow) {
    app.set_wsl_manage_strings(WslManageStrings {
        status_title: i18n::t("wsl_manage.status_title").into(),
        status_running: i18n::t("wsl_manage.status_running").into(),
        status_stopped: i18n::t("wsl_manage.status_stopped").into(),
        status_not_installed: i18n::t("wsl_manage.status_not_installed").into(),
        default_version_title: i18n::t("wsl_manage.default_version_title").into(),
        default_version_desc: i18n::t("wsl_manage.default_version_desc").into(),
        actions_title: i18n::t("wsl_manage.actions_title").into(),
        update_btn: i18n::t("wsl_manage.update_btn").into(),
        updating_btn: i18n::t("wsl_manage.updating_btn").into(),
        shutdown_btn: i18n::t("wsl_manage.shutdown_btn").into(),
        shutdown_btn_tooltip: i18n::t("wsl_manage.shutdown_btn_tooltip").into(),
        shutdown_confirm_title: i18n::t("wsl_manage.shutdown_confirm_title").into(),
        shutdown_confirm_message: i18n::t("wsl_manage.shutdown_confirm_message").into(),
        start_btn_tooltip: i18n::t("wsl_manage.start_btn_tooltip").into(),
        version_output_title: i18n::t("wsl_manage.version_output_title").into(),
        status_output_title: i18n::t("wsl_manage.status_output_title").into(),
        uninstall_btn: i18n::t("wsl_manage.uninstall_btn").into(),
        settings_btn: i18n::t("wsl_manage.settings_btn").into(),
        version_label_1: i18n::t("wsl_manage.version_label_1").into(),
        version_label_2: i18n::t("wsl_manage.version_label_2").into(),
        sparse_mode: i18n::t("wsl_manage.sparse_mode").into(),
    });
}

fn extract_version_number(raw: &str) -> String {
    if let Some(pos) = raw.find(':') {
        raw[pos + 1..].trim().to_string()
    } else {
        raw.to_string()
    }
}
