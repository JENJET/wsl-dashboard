use crate::i18n;
use crate::ui::data;
use crate::{AppState, AppWindow, WslManageStrings};
use slint::{ModelRc, VecModel};
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
    app.on_wsl_manage_install(move || {
        let ah = ah.clone();
        let _ = slint::spawn_local(async move {
            let mut cmd = std::process::Command::new("wsl.exe");
            cmd.arg("--install");
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NEW_CONSOLE: u32 = 0x00000010;
                cmd.creation_flags(CREATE_NEW_CONSOLE);
            }
            match cmd.spawn() {
                Ok(_) => {
                    info!("WSL install command launched");
                    let msg = i18n::t("wsl_manage.install_success");
                    data::show_message(&ah, &msg);
                }
                Err(e) => {
                    error!("Failed to launch WSL install: {}", e);
                    let msg = i18n::tr("wsl_manage.install_failed", &[e.to_string()]);
                    data::show_message(&ah, &msg);
                }
            }
        });
    });

    // Start WSL (open default distro terminal)
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_manage_start(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let _ = slint::spawn_local(async move {
            let mut cmd = std::process::Command::new("wsl.exe");
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NEW_CONSOLE: u32 = 0x00000010;
                cmd.creation_flags(CREATE_NEW_CONSOLE);
            }
            match cmd.spawn() {
                Ok(_) => {
                    debug!("WSL terminal launched");
                    // Wait for instance to start, then refresh status
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    refresh_wsl_info(&ah, &as_ptr).await;
                }
                Err(e) => {
                    error!("Failed to launch WSL terminal: {}", e);
                }
            }
        });
    });

    // Update WSL
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_manage_update(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let _ = slint::spawn_local(async move {
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

            if let Some(app) = ah.upgrade() {
                app.set_wsl_update_confirm_message(confirm_message.into());
                app.set_show_wsl_update_confirm(true);
            }
        });
    });

    // WSL Update Confirmed (user clicked OK on confirmation dialog)
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_manage_update_confirmed(move |preview| {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let _ = slint::spawn_local(async move {
            if let Some(app) = ah.upgrade() {
                app.set_wsl_is_updating(true);
            }

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

            if let Some(app) = ah.upgrade() {
                app.set_wsl_is_updating(false);
            }

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
            let executor = {
                let state = as_ptr.lock().await;
                state.wsl_dashboard.executor().clone()
            };

            let result = executor.execute_command(&["--shutdown"]).await;

            if result.success {
                info!("WSL shutdown all completed");
                let msg = i18n::t("wsl_manage.shutdown_success");
                data::show_message(&ah, &msg);
                let state = as_ptr.lock().await;
                let _ = state.wsl_dashboard.refresh_distros().await;
                drop(state);
                refresh_wsl_info(&ah, &as_ptr).await;
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
    app.on_wsl_manage_set_default_version(move |version| {
        let ah = ah.clone();
        let _ = slint::spawn_local(async move {
            let mut cmd = std::process::Command::new("wsl.exe");
            cmd.arg("--set-default-version").arg(version.to_string());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());

            match cmd.output() {
                Ok(output) => {
                    if output.status.success() {
                        info!("WSL default version set to {}", version);
                        if let Some(app) = ah.upgrade() {
                            app.set_wsl_default_version(version);
                        }
                        let msg = i18n::t("wsl_manage.set_default_success");
                        data::show_message(&ah, &msg);
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        error!("Failed to set WSL default version: {}", stderr);
                        let msg = i18n::tr("wsl_manage.set_default_failed", &[stderr.to_string()]);
                        data::show_message(&ah, &msg);
                    }
                }
                Err(e) => {
                    error!("Failed to execute set-default-version: {}", e);
                    let msg = i18n::tr("wsl_manage.set_default_failed", &[e.to_string()]);
                    data::show_message(&ah, &msg);
                }
            }
        });
    });

    // Uninstall WSL
    let ah = app_handle.clone();
    app.on_wsl_manage_uninstall(move || {
        let ah = ah.clone();
        let _ = slint::spawn_local(async move {
            let mut cmd = std::process::Command::new("wsl.exe");
            cmd.arg("--uninstall");
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                // Needs admin privileges, show console for user interaction
                const CREATE_NEW_CONSOLE: u32 = 0x00000010;
                cmd.creation_flags(CREATE_NEW_CONSOLE);
            }
            match cmd.spawn() {
                Ok(_) => {
                    info!("WSL uninstall command launched");
                    let msg = i18n::t("wsl_manage.uninstall_success");
                    data::show_message(&ah, &msg);
                }
                Err(e) => {
                    error!("Failed to launch WSL uninstall: {}", e);
                    let msg = i18n::tr("wsl_manage.uninstall_failed", &[e.to_string()]);
                    data::show_message(&ah, &msg);
                }
            }
        });
    });

    // Open WSL Settings (directly launch wslsettings.exe)
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_manage_open_settings(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let _ = slint::spawn_local(async move {
            let state = as_ptr.lock().await;
            let executor = state.wsl_dashboard.executor().clone();
            drop(state);

            let show_upgrade_prompt = |app: slint::Weak<AppWindow>| {
                if let Some(app) = app.upgrade() {
                    app.set_current_message(i18n::t("settings.wsl2_required").into());
                    app.set_current_message_link(i18n::t("settings.update_wsl").into());
                    app.set_current_message_url(
                        "https://github.com/microsoft/WSL/releases/latest".into(),
                    );
                    app.set_show_message_dialog(true);
                }
            };

            // Check if it's the Store version (which supports WSL Settings)
            let version_check = executor.execute_command(&["--version"]).await;
            if !version_check.success {
                show_upgrade_prompt(ah);
                return;
            }

            // Discover wslsettings.exe path
            let rel_path = "Program Files\\WSL\\wslsettings\\wslsettings.exe";
            let mut exe_path = std::path::PathBuf::from(format!("C:\\{}", rel_path));
            let mut found = exe_path.exists();

            if !found {
                if let Ok(system_drive) = std::env::var("SystemDrive") {
                    if system_drive.to_uppercase() != "C:" {
                        let alt_path =
                            std::path::PathBuf::from(format!("{}\\{}", system_drive, rel_path));
                        if alt_path.exists() {
                            exe_path = alt_path;
                            found = true;
                        }
                    }
                }
            }

            if !found {
                for drive in b'C'..=b'Z' {
                    let drive_str = format!("{}:", drive as char);
                    let alt_path = std::path::PathBuf::from(format!("{}\\{}", drive_str, rel_path));
                    if alt_path.exists() {
                        exe_path = alt_path;
                        found = true;
                        break;
                    }
                }
            }

            if found {
                let mut cmd = std::process::Command::new(exe_path);
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x08000000;
                    cmd.creation_flags(CREATE_NO_WINDOW);
                }
                let _ = cmd.spawn().map_err(|e| {
                    error!("Failed to launch WSL settings: {}", e);
                });
            } else {
                show_upgrade_prompt(ah);
            }
        });
    });
}

/// Split command output into a Slint-compatible `[string]` model
fn output_to_model(output: &str) -> ModelRc<slint::SharedString> {
    let lines: Vec<slint::SharedString> = output.lines().map(|l| l.into()).collect();
    ModelRc::new(VecModel::from(lines))
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

    // Check if any distros are running
    let has_running = {
        let state = app_state.lock().await;
        let distros = state.wsl_dashboard.get_distros().await;
        distros
            .iter()
            .any(|d| matches!(d.status, crate::wsl::models::WslStatus::Running))
    };

    let ah = app_handle.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = ah.upgrade() {
            if version_result.success {
                app.set_wsl_installed(true);

                // Pass raw output lines to the UI
                let version_lines = output_to_model(&version_result.output);
                app.set_wsl_version_output(version_lines);

                // Still parse default_version from status for the toggle buttons
            } else {
                app.set_wsl_installed(false);
                app.set_wsl_version_output(ModelRc::new(VecModel::from(
                    Vec::<slint::SharedString>::new(),
                )));
                debug!("WSL --version failed, WSL may not be installed or is inbox version");
            }

            if status_result.success {
                let status_lines = output_to_model(&status_result.output);
                app.set_wsl_status_output(status_lines);

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
                            } else if val.contains("2") {
                                app.set_wsl_default_version(2);
                            } else {
                                app.set_wsl_default_version(1);
                            }
                        }
                    }
                }
            } else {
                app.set_wsl_status_output(ModelRc::new(VecModel::from(
                    Vec::<slint::SharedString>::new(),
                )));
            }

            app.set_wsl_has_running(has_running);
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
        version_output_title: i18n::t("wsl_manage.version_output_title").into(),
        status_output_title: i18n::t("wsl_manage.status_output_title").into(),
        uninstall_btn: i18n::t("wsl_manage.uninstall_btn").into(),
        settings_btn: i18n::t("wsl_manage.settings_btn").into(),
        version_label_1: i18n::t("wsl_manage.version_label_1").into(),
        version_label_2: i18n::t("wsl_manage.version_label_2").into(),
    });
}

fn extract_version_number(raw: &str) -> String {
    if let Some(pos) = raw.find(':') {
        raw[pos + 1..].trim().to_string()
    } else {
        raw.to_string()
    }
}
