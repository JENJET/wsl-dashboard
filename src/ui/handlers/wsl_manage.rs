use crate::i18n;
use crate::ui::data;
use crate::utils::system::CREATE_NO_WINDOW;
use crate::wsl::models::{MountedDisk, WslCommandResult};
use crate::{AppState, AppWindow, WslManageStrings};
use std::os::windows::process::CommandExt;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info};

fn update_streaming_output(app: &AppWindow, text: &str) {
    let cleaned = text.replace('\r', "");
    if cleaned.is_empty() {
        return;
    }
    let mut new_output = app.get_wsl_streaming_output().to_string();
    new_output.push_str(&cleaned);
    app.set_wsl_streaming_output(new_output.into());
}

pub fn setup(app: &AppWindow, app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    // Refresh WSL info when the tab is first opened
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_manage_refresh(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let _ = slint::spawn_local(async move {
            refresh_wsl_info(&ah, &as_ptr).await;
            refresh_physical_disks_inner(&ah, &as_ptr).await;
        });
    });

    // Install WSL (elevated streaming)
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_manage_install(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        tokio::spawn(async move {
            let ah2 = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah2.upgrade() {
                    app.set_wsl_streaming_title(i18n::t("wsl_manage.installing_title").into());
                    app.set_wsl_streaming_output(i18n::t("dialog.processing").into());
                    app.set_wsl_streaming_is_error(false);
                    app.set_wsl_streaming_running(true);
                    app.set_show_wsl_streaming(true);
                }
            });

            let executor = {
                let state = as_ptr.lock().await;
                state.wsl_dashboard.executor().clone()
            };

            // 一次提权完成全部操作：启用 VMP → 启用 WSL → 安装（不下载发行版）
            info!("Enabling WSL system components via DISM");
            let ah_msg = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_msg.upgrade() {
                    app.set_wsl_streaming_output(
                        i18n::t("wsl_manage.enabling_wsl_features").into(),
                    );
                }
            });

            // 用临时批处理文件执行，避免 cmd.exe /c & 链的解析歧义
            let bat_content = "\
@echo off\r\n\
dism.exe /online /enable-feature /featurename:VirtualMachinePlatform /all /norestart\r\n\
if errorlevel 3010 set REBOOT=1\r\n\
if errorlevel 1 if not errorlevel 3010 set FAIL=1\r\n\
dism.exe /online /enable-feature /featurename:Microsoft-Windows-Subsystem-Linux /all /norestart\r\n\
if errorlevel 3010 set REBOOT=1\r\n\
if errorlevel 1 if not errorlevel 3010 set FAIL=1\r\n\
wsl.exe --install --no-distribution\r\n\
if defined REBOOT exit /b 3010\r\n\
if defined FAIL exit /b 1\r\n\
";
            let bat_path = std::env::temp_dir().join("wsl_dashboard_install.bat");
            let result = match std::fs::write(&bat_path, bat_content) {
                Ok(()) => {
                    let bat_str = bat_path.to_string_lossy().to_string();
                    let r = executor
                        .execute_command_elevated_streaming(
                            "cmd.exe",
                            &["/c", &bat_str],
                            true,
                            |_| {},
                        )
                        .await;
                    let _ = std::fs::remove_file(&bat_path);
                    r
                }
                Err(e) => {
                    error!("Failed to create temp batch file: {}", e);
                    WslCommandResult::error(
                        String::new(),
                        format!("Cannot create temp script: {}", e),
                    )
                }
            };

            let success = result.success;
            let error_msg = result.error.unwrap_or_default();
            let reboot = result.output == "REBOOT_REQUIRED";
            if reboot {
                let ah2 = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah2.upgrade() {
                        app.set_wsl_streaming_running(false);
                        app.set_wsl_streaming_is_error(false);
                        update_streaming_output(
                            &app,
                            &format!("\n{}", i18n::t("wsl_manage.enable_feature_reboot")),
                        );
                        app.set_show_reboot_confirm(true);
                    }
                });
            } else if success {
                let ah2 = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah2.upgrade() {
                        app.set_wsl_streaming_running(false);
                        update_streaming_output(
                            &app,
                            &format!("\n{}", i18n::t("wsl_manage.install_success")),
                        );
                    }
                });
            } else {
                let ah2 = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah2.upgrade() {
                        app.set_wsl_streaming_running(false);
                        app.set_wsl_streaming_is_error(true);
                        update_streaming_output(
                            &app,
                            &format!(
                                "\n{}",
                                i18n::tr("wsl_manage.install_failed", &[error_msg.clone()])
                            ),
                        );
                    }
                });
            }

            refresh_wsl_info(&ah, &as_ptr).await;
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

    // WSL Update Confirmed (elevated streaming)
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_manage_update_confirmed(move |preview| {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        tokio::spawn(async move {
            // Show streaming dialog
            let ah2 = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah2.upgrade() {
                    app.set_wsl_is_updating(true);
                    app.set_wsl_streaming_title(i18n::t("wsl_manage.updating_title").into());
                    app.set_wsl_streaming_output(i18n::t("dialog.processing").into());
                    app.set_wsl_streaming_is_error(false);
                    app.set_wsl_streaming_running(true);
                    app.set_show_wsl_streaming(true);
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

            let ah_cb = ah.clone();
            let result = executor
                .execute_command_elevated_streaming("wsl.exe", &args, true, move |text| {
                    let ah = ah_cb.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah.upgrade() {
                            update_streaming_output(&app, &text);
                        }
                    });
                })
                .await;

            let success = result.success;
            let output = result.output.clone();
            let error_msg = result.error.unwrap_or_default();
            let status_msg = if success {
                let output_lower = output.to_lowercase();
                if output_lower.contains("no update") || output_lower.contains("already") {
                    i18n::t("wsl_manage.update_already_latest")
                } else {
                    i18n::t("wsl_manage.update_success")
                }
            } else {
                i18n::tr("wsl_manage.update_failed", &[error_msg.clone()])
            };
            let ah2 = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah2.upgrade() {
                    app.set_wsl_is_updating(false);
                    app.set_wsl_streaming_running(false);
                    app.set_wsl_streaming_is_error(!success);
                    update_streaming_output(&app, &format!("\n{}", status_msg));
                }
            });

            if success {
                info!("WSL update completed successfully");
            } else {
                error!("WSL update failed: {}", error_msg);
            }
            refresh_wsl_info(&ah, &as_ptr).await;
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

    // Uninstall WSL (confirmation dialog shown in Slint, just log here)
    app.on_wsl_manage_uninstall(|| {
        info!("WSL uninstall confirmation requested");
    });

    // Uninstall WSL Confirmed (elevated streaming)
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_manage_uninstall_confirmed(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        tokio::spawn(async move {
            let ah2 = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah2.upgrade() {
                    app.set_wsl_streaming_title(i18n::t("wsl_manage.uninstalling_title").into());
                    app.set_wsl_streaming_output(i18n::t("dialog.processing").into());
                    app.set_wsl_streaming_is_error(false);
                    app.set_wsl_streaming_running(true);
                    app.set_show_wsl_streaming(true);
                }
            });

            let executor = {
                let state = as_ptr.lock().await;
                state.wsl_dashboard.executor().clone()
            };

            let ah_cb = ah.clone();
            let result = executor
                .execute_command_elevated_streaming(
                    "wsl.exe",
                    &["--uninstall"],
                    false,
                    move |text| {
                        let ah = ah_cb.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah.upgrade() {
                                update_streaming_output(&app, &text);
                            }
                        });
                    },
                )
                .await;

            let success = result.success;
            let error_msg = result.error.unwrap_or_default();
            let msg = if success {
                i18n::t("wsl_manage.uninstall_success")
            } else {
                i18n::tr("wsl_manage.uninstall_failed", &[error_msg.clone()])
            };
            let ah2 = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah2.upgrade() {
                    app.set_wsl_streaming_running(false);
                    app.set_wsl_streaming_is_error(!success);
                    update_streaming_output(&app, &format!("\n{}", msg));
                }
            });

            if success {
                info!("WSL uninstall completed successfully");
            } else {
                error!("WSL uninstall failed: {}", error_msg);
            }

            // 直接清空缓存并更新 UI，不跑 wsl 命令（避免延迟）
            {
                let state = as_ptr.lock().await;
                let mut distros_lock = state.wsl_dashboard.distros.lock().await;
                *distros_lock = Vec::new();
                state.wsl_dashboard.state_changed().notify_one();
            }
            let ah2 = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah2.upgrade() {
                    app.set_wsl_installed(false);
                    app.set_wsl_has_instances(false);
                    app.set_wsl_has_running(false);
                    app.set_wsl_version_output("".into());
                    app.set_wsl_status_output("".into());
                    app.set_wsl_default_distro_name("".into());
                }
            });
        });
    });

    // Close WSL streaming dialog
    let ah = app_handle.clone();
    app.on_wsl_streaming_close(move || {
        if let Some(app) = ah.upgrade() {
            app.set_show_wsl_streaming(false);
        }
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

    // Reboot system handler (from reboot confirm dialog)
    {
        app.on_reboot_system(move || {
            std::thread::spawn(move || {
                info!("User confirmed reboot, restarting system...");
                let args = vec!["/r".to_string(), "/t".to_string(), "0".to_string()];
                let _ =
                    crate::utils::system::run_elevated_and_wait("shutdown.exe", args, true, None);
            });
        });
    }

    // Cancel reboot handler
    {
        app.on_cancel_reboot(move || {
            info!("User cancelled reboot");
        });
    }

    // === Disk Mount Handlers ===

    // Mount disk
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_mount_disk(
        move |disk, _is_vhd, is_bare, name, fs_type, partition, options| {
            let ah = ah.clone();
            let as_ptr = as_ptr.clone();
            tokio::spawn(async move {
                let disk_str = disk.to_string();
                let is_vhd = disk_str.to_lowercase().ends_with(".vhd")
                    || disk_str.to_lowercase().ends_with(".vhdx");
                let mut name_str = name.to_string();
                let fs_type_str = fs_type.to_string();
                let partition_str = partition.to_string();
                let options_str = options.to_string();

                // Auto-fill mount name from VHD filename if not specified
                if name_str.is_empty() && is_vhd {
                    if let Some(stem) = std::path::Path::new(&disk_str).file_stem() {
                        name_str = stem.to_string_lossy().to_string();
                    }
                }

                // Show streaming dialog
                let ah2 = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah2.upgrade() {
                        app.set_wsl_mount_running(true);
                        app.set_wsl_mount_output(i18n::t("dialog.processing").into());
                        app.set_wsl_mount_is_error(false);
                    }
                });

                let executor = {
                    let state = as_ptr.lock().await;
                    state.wsl_dashboard.executor().clone()
                };

                let mut result = executor
                    .mount_disk(
                        &disk_str,
                        is_vhd,
                        is_bare,
                        &name_str,
                        &fs_type_str,
                        &partition_str,
                        &options_str,
                    )
                    .await;

                // If mount failed due to missing elevation, retry elevated
                if !result.success {
                    let err = result.error.as_deref().unwrap_or("");
                    let err_lower = err.to_lowercase();
                    let needs_elevation = err_lower.contains("elevation")
                        || err_lower.contains("elevated")
                        || err_lower.contains("access denied")
                        || err_lower.contains("access_denied")
                        || err_lower.contains("admin")
                        || err_lower.contains("permission");
                    if needs_elevation {
                        info!("Mount requires elevation, retrying via elevated wsl.exe...");
                        result = crate::wsl::ops::disk_mount::mount_disk_elevated(
                            &disk_str,
                            is_vhd,
                            is_bare,
                            &name_str,
                            &fs_type_str,
                            &partition_str,
                            &options_str,
                        )
                        .await;
                    }
                }

                let success = result.success;
                let error_msg = result.error.unwrap_or_default();

                // Track mounted disk in state on success
                if success {
                    let mut state = as_ptr.lock().await;
                    state.mounted_disks.push(MountedDisk {
                        disk: disk_str.clone(),
                        mount_name: name_str.clone(),
                        filesystem: fs_type_str.clone(),
                    });
                }

                let msg = if success {
                    i18n::t("wsl_manage.mount_success")
                } else {
                    let error_lower = error_msg.to_lowercase();
                    let hint = if error_lower.contains("no such device")
                        || error_lower.contains("装载失败")
                    {
                        i18n::t("wsl_manage.mount_hint_no_such_device")
                    } else {
                        i18n::t("wsl_manage.mount_hint_generic")
                    };
                    format!(
                        "{}\n{}",
                        i18n::tr("wsl_manage.mount_failed", &[error_msg.clone()]),
                        hint
                    )
                };

                let ah2 = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah2.upgrade() {
                        app.set_wsl_mount_running(false);
                        app.set_wsl_mount_is_error(!success);
                        app.set_wsl_mount_output(msg.into());
                    }
                });

                // Refresh mounted disks
                refresh_mounted_disks(&ah, &as_ptr).await;
            });
        },
    );

    // Unmount disk
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_unmount_disk(move |disk| {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let disk_display = disk.to_string();
        // Parse display string "path (filesystem)" → "path"
        let disk_path = if let Some(pos) = disk_display.find(" (") {
            disk_display[..pos].to_string()
        } else {
            disk_display.clone()
        };
        tokio::spawn(async move {
            let executor = {
                let state = as_ptr.lock().await;
                state.wsl_dashboard.executor().clone()
            };

            let result = executor.unmount_disk(&disk_path).await;

            let success = result.success;
            let error_msg = result.error.unwrap_or_default();

            // Remove from tracked state on success
            if success {
                let mut state = as_ptr.lock().await;
                state.mounted_disks.retain(|d| d.disk != disk_path);
            }

            let msg = if success {
                i18n::t("wsl_manage.unmount_success")
            } else {
                i18n::tr("wsl_manage.unmount_failed", &[error_msg.clone()])
            };

            let ah2 = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah2.upgrade() {
                    app.set_wsl_mount_output(msg.into());
                    app.set_wsl_mount_is_error(!success);
                }
            });

            refresh_mounted_disks(&ah, &as_ptr).await;
        });
    });

    // Unmount all disks
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_unmount_all_disks(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        tokio::spawn(async move {
            let executor = {
                let state = as_ptr.lock().await;
                state.wsl_dashboard.executor().clone()
            };

            let result = executor.unmount_disk("").await;

            let success = result.success;
            let error_msg = result.error.unwrap_or_default();

            // Clear tracked state on success
            if success {
                let mut state = as_ptr.lock().await;
                state.mounted_disks.clear();
            }

            let msg = if success {
                i18n::t("wsl_manage.unmount_success")
            } else {
                i18n::tr("wsl_manage.unmount_failed", &[error_msg.clone()])
            };

            let ah2 = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah2.upgrade() {
                    app.set_wsl_mount_output(msg.into());
                    app.set_wsl_mount_is_error(!success);
                }
            });

            refresh_mounted_disks(&ah, &as_ptr).await;
        });
    });

    // Refresh mounted disks
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_refresh_mounted_disks(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        tokio::spawn(async move {
            refresh_mounted_disks(&ah, &as_ptr).await;
        });
    });

    // Refresh physical disks
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_wsl_refresh_physical_disks(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        tokio::spawn(async move {
            refresh_physical_disks_inner(&ah, &as_ptr).await;
        });
    });

    // Browse VHD file
    {
        app.on_wsl_browse_vhd(move || {
            if let Some(path) = rfd::FileDialog::new()
                .set_title(i18n::t("wsl_manage.select_vhd_file"))
                .add_filter("VHD(X)", &["vhd", "vhdx"])
                .pick_file()
            {
                if let Some(app) = app_handle.upgrade() {
                    let path_str = path.display().to_string();
                    app.set_wsl_disk_path(path_str.into());
                    if let Some(stem) = path.file_stem() {
                        app.set_wsl_disk_mount_name(stem.to_string_lossy().to_string().into());
                    }
                }
            }
        });
    }

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
                                let is_rtl = crate::i18n::is_rtl(&crate::i18n::current_lang());
                                if is_rtl {
                                    lines.push(format!("%{:.2} :{}", cpu_val, cpu_label).into());
                                } else {
                                    lines.push(format!("{}: {:.2}%", cpu_label, cpu_val).into());
                                }
                            }
                            let mem_label = i18n::t("distro.memory_tooltip");
                            let is_rtl = crate::i18n::is_rtl(&crate::i18n::current_lang());
                            if mem_mb >= 1024.0 {
                                if is_rtl {
                                    lines.push(format!("{:.2} GB :{}", mem_mb / 1024.0, mem_label).into());
                                } else {
                                    lines.push(format!("{}: {:.2} GB", mem_label, mem_mb / 1024.0).into());
                                }
                            } else {
                                if is_rtl {
                                    lines.push(format!("{:.2} MB :{}", mem_mb, mem_label).into());
                                } else {
                                    lines.push(format!("{}: {:.2} MB", mem_label, mem_mb).into());
                                }
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

    // Also refresh mounted disks
    refresh_mounted_disks(app_handle, app_state).await;
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
        install_btn: i18n::t("wsl_manage.install_btn").into(),
        update_btn: i18n::t("wsl_manage.update_btn").into(),
        updating_btn: i18n::t("wsl_manage.updating_btn").into(),
        update_desc: i18n::t("wsl_manage.update_desc").into(),
        shutdown_btn: i18n::t("wsl_manage.shutdown_btn").into(),
        shutdown_btn_tooltip: i18n::t("wsl_manage.shutdown_btn_tooltip").into(),
        shutdown_confirm_title: i18n::t("wsl_manage.shutdown_confirm_title").into(),
        shutdown_confirm_message: i18n::t("wsl_manage.shutdown_confirm_message").into(),
        start_btn: i18n::t("wsl_manage.start_btn").into(),
        start_btn_tooltip: i18n::t("wsl_manage.start_btn_tooltip").into(),
        version_output_title: i18n::t("wsl_manage.version_output_title").into(),
        status_output_title: i18n::t("wsl_manage.status_output_title").into(),
        uninstall_btn: i18n::t("wsl_manage.uninstall_btn").into(),
        uninstall_desc: i18n::t("wsl_manage.uninstall_desc").into(),
        settings_btn: i18n::t("wsl_manage.settings_btn").into(),
        settings_desc: i18n::t("wsl_manage.settings_desc").into(),
        version_label_1: i18n::t("wsl_manage.version_label_1").into(),
        version_label_2: i18n::t("wsl_manage.version_label_2").into(),
        sparse_mode: i18n::t("wsl_manage.sparse_mode").into(),
        // Disk mount
        disk_management_title: i18n::t("wsl_manage.disk_management_title").into(),
        mount_disk_title: i18n::t("wsl_manage.mount_disk_title").into(),
        physical_disk_label: i18n::t("wsl_manage.physical_disk_label").into(),
        usb_mount_hint: i18n::t("wsl_manage.usb_mount_hint").into(),
        disk_path_label: i18n::t("wsl_manage.disk_path_label").into(),
        disk_path_placeholder: i18n::t("wsl_manage.disk_path_placeholder").into(),
        is_vhd_label: i18n::t("wsl_manage.is_vhd_label").into(),
        bare_mount_label: i18n::t("wsl_manage.bare_mount_label").into(),
        mount_name_label: i18n::t("wsl_manage.mount_name_label").into(),
        mount_name_placeholder: i18n::t("wsl_manage.mount_name_placeholder").into(),
        filesystem_label: i18n::t("wsl_manage.filesystem_label").into(),
        filesystem_placeholder: i18n::t("wsl_manage.filesystem_placeholder").into(),
        partition_label: i18n::t("wsl_manage.partition_label").into(),
        partition_placeholder: i18n::t("wsl_manage.partition_placeholder").into(),
        mount_options_label: i18n::t("wsl_manage.mount_options_label").into(),
        mount_options_placeholder: i18n::t("wsl_manage.mount_options_placeholder").into(),
        mount_btn: i18n::t("wsl_manage.mount_btn").into(),
        mounting_btn: i18n::t("wsl_manage.mounting_btn").into(),
        mounted_disks_title: i18n::t("wsl_manage.mounted_disks_title").into(),
        unmount_btn: i18n::t("wsl_manage.unmount_btn").into(),
        unmount_all_btn: i18n::t("wsl_manage.unmount_all_btn").into(),
        no_mounted_disks: i18n::t("wsl_manage.no_mounted_disks").into(),
        no_mounted_disks_hint: i18n::t("wsl_manage.no_mounted_disks_hint").into(),
        refresh_disks_btn: i18n::t("wsl_manage.refresh_disks_btn").into(),
        mount_success: i18n::t("wsl_manage.mount_success").into(),
        mount_failed: i18n::t("wsl_manage.mount_failed").into(),
        unmount_success: i18n::t("wsl_manage.unmount_success").into(),
        unmount_failed: i18n::t("wsl_manage.unmount_failed").into(),
        disk_label: i18n::t("wsl_manage.disk_label").into(),
    });
}

fn extract_version_number(raw: &str) -> String {
    if let Some(pos) = raw.find(':') {
        raw[pos + 1..].trim().to_string()
    } else {
        raw.to_string()
    }
}

/// Refresh mounted disks list and update UI
async fn refresh_mounted_disks(
    app_handle: &slint::Weak<AppWindow>,
    app_state: &Arc<Mutex<AppState>>,
) {
    let mounted = {
        let state = app_state.lock().await;
        state.mounted_disks.clone()
    };

    let mounted_strs: Vec<slint::SharedString> = mounted
        .iter()
        .map(|d| {
            if d.filesystem.is_empty() {
                d.disk.clone().into()
            } else {
                format!("{} ({})", d.disk, d.filesystem).into()
            }
        })
        .collect();

    let ah = app_handle.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = ah.upgrade() {
            let model = slint::ModelRc::new(slint::VecModel::from(mounted_strs));
            app.set_wsl_mounted_disks(model);
        }
    });
}

pub async fn refresh_physical_disks_inner(
    app_handle: &slint::Weak<AppWindow>,
    app_state: &Arc<Mutex<AppState>>,
) {
    let executor = {
        let state = app_state.lock().await;
        state.wsl_dashboard.executor().clone()
    };

    let result = executor.list_physical_disks().await;
    if let Some(mut disks) = result.data {
        // Exclude USB disks (USB bus type cannot be mounted by WSL2)
        disks.retain(|d| d.bus_type.to_lowercase() != "usb");
        // Sort by disk number ascending
        disks.sort_by_key(|d| d.number);
        let disk_strs: Vec<slint::SharedString> = disks
            .iter()
            .map(|d| format!("\\\\.\\PHYSICALDRIVE{} — {}", d.number, d.friendly_name).into())
            .collect();
        let path_strs: Vec<slint::SharedString> = disks
            .iter()
            .map(|d| format!("\\\\.\\PHYSICALDRIVE{}", d.number).into())
            .collect();
        let name_strs: Vec<slint::SharedString> = disks
            .iter()
            .map(|d| d.friendly_name.clone().into())
            .collect();
        let ah = app_handle.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = ah.upgrade() {
                let display_model = slint::ModelRc::new(slint::VecModel::from(disk_strs));
                let path_model = slint::ModelRc::new(slint::VecModel::from(path_strs));
                let name_model = slint::ModelRc::new(slint::VecModel::from(name_strs));
                app.set_wsl_physical_disks(display_model);
                app.set_wsl_physical_disks_paths(path_model);
                app.set_wsl_physical_disks_names(name_model);
            }
        });
    }
}
