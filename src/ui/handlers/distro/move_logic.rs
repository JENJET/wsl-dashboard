use crate::ui::data::refresh_distros_ui;
use crate::{AppState, AppWindow, i18n};
use scopeguard;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;
use tracing::{info, warn};

async fn do_move_inner(
    ah: slint::Weak<AppWindow>,
    as_ptr: Arc<Mutex<AppState>>,
    source_name: &str,
    target_name: &str,
    target_path: &str,
    version: &str,
    use_elevation: bool,
) -> (crate::wsl::models::WslCommandResult<String>, Option<String>) {
    let dashboard = {
        let state = as_ptr.lock().await;
        state.wsl_dashboard.clone()
    };
    dashboard.set_manual_operation(true);
    let _manual_op_guard = scopeguard::guard((), |_| {
        dashboard.set_manual_operation(false);
    });

    if version == crate::wsl::models::WslVersion::V2.to_string() {
        let _ = dashboard.shutdown_wsl().await;
        tokio::time::sleep(std::time::Duration::from_millis(3000)).await;
    } else {
        let _ = dashboard.stop_distro(source_name).await;
        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
    }

    let old_install_location = dashboard
        .executor()
        .get_distro_install_location(source_name)
        .await
        .data;

    let result = if version == crate::wsl::models::WslVersion::V2.to_string() {
        let disk_name = old_install_location
            .as_deref()
            .and_then(|dir| std::fs::read_dir(dir).ok())
            .and_then(|mut entries| {
                entries.find_map(|e| {
                    e.ok().and_then(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        if name.ends_with(".vhdx") || name.ends_with(".vhd") {
                            Some(name)
                        } else {
                            None
                        }
                    })
                })
            })
            .unwrap_or_else(|| "ext4.vhdx".to_string());
        let vhdx_path = std::path::Path::new(target_path).join(&disk_name);
        let stop_signal = Arc::new(AtomicBool::new(false));
        super::spawn_file_size_monitor(
            ah.clone(),
            vhdx_path.to_string_lossy().to_string(),
            source_name.to_string(),
            "operation.moving_wsl2_msg".to_string(),
            stop_signal.clone(),
            None,
        );

        let mut move_res = crate::wsl::models::WslCommandResult::error("".into(), "".into());
        for attempt in 1..=3 {
            if attempt > 1 {
                info!("WSL 2 Move: Retry attempt {} after delay...", attempt);
                tokio::time::sleep(std::time::Duration::from_millis(3000)).await;
            }
            tokio::task::yield_now().await;
            move_res = dashboard
                .move_distro(source_name, target_path, use_elevation)
                .await;
            if move_res.success {
                break;
            }
            if move_res.output.contains("ERROR_SHARING_VIOLATION")
                || move_res.output.contains("0x80070020")
            {
                continue;
            } else {
                break;
            }
        }
        stop_signal.store(true, Ordering::SeqCst);
        move_res
    } else {
        move_wsl1(
            ah.clone(),
            as_ptr.clone(),
            source_name,
            target_name,
            target_path,
        )
        .await
    };

    (result, old_install_location)
}

pub fn run_move_process(
    ah_move: slint::Weak<AppWindow>,
    as_ptr: Arc<Mutex<AppState>>,
    source_name: String,
    target_name: String,
    target_path: String,
    version: String,
    use_elevation: bool,
) {
    let _ = slint::spawn_local(async move {
        if let Some(app) = ah_move.upgrade() {
            app.set_task_status_text(i18n::t("operation.moving").into());
            app.set_task_status_visible(true);
        }

        let (result, old_install_location) = do_move_inner(
            ah_move.clone(),
            as_ptr.clone(),
            &source_name,
            &target_name,
            &target_path,
            &version,
            use_elevation,
        )
        .await;

        if let Some(app) = ah_move.upgrade() {
            app.set_task_status_visible(false);
            app.set_is_moving(false);
            if result.success {
                if let Some(ref src_path) = old_install_location {
                    let ico_src = std::path::Path::new(src_path).join("shortcut.ico");
                    if ico_src.exists() {
                        let ico_dst = std::path::Path::new(&target_path).join("shortcut.ico");
                        if ico_src != ico_dst {
                            info!("Moving shortcut.ico from {:?} to {:?}", ico_src, ico_dst);
                            let _ = std::fs::copy(&ico_src, &ico_dst);
                        }
                    }
                    let _ = std::fs::remove_dir_all(src_path);
                }

                app.set_current_message(
                    i18n::tr("dialog.move_success", &[source_name, target_path]).into(),
                );
            } else {
                let err = result.error.unwrap_or_else(|| i18n::t("dialog.error"));
                if result.output == "BACKUP_SAVED" {
                    app.set_current_message(
                        i18n::tr("dialog.move_failed_backup", &[err.clone()]).into(),
                    );
                    app.set_current_message_link(i18n::t("distro.explorer").into());
                    let backup_path = std::path::Path::new(&err);
                    if let Some(parent) = backup_path.parent() {
                        app.set_current_message_url(parent.to_string_lossy().to_string().into());
                    } else {
                        app.set_current_message_url(err.into());
                    }
                } else {
                    app.set_current_message(i18n::tr("dialog.move_failed", &[err]).into());
                }
            }
            app.set_show_message_dialog(true);
            refresh_distros_ui(ah_move.clone(), as_ptr.clone()).await;
        }
    });
}

async fn move_wsl1(
    ah: slint::Weak<AppWindow>,
    as_ptr: Arc<Mutex<AppState>>,
    source_name: &str,
    target_name: &str,
    target_path: &str,
) -> crate::wsl::models::WslCommandResult<String> {
    use crate::wsl::models::WslCommandResult;

    let (temp_dir, temp_file_str) =
        super::resolve_temp_path(as_ptr.clone(), source_name, "wsl_move", "tar").await;
    let _ = std::fs::create_dir_all(&temp_dir);

    info!(
        "WSL1 Move: Exporting '{}' to '{}'...",
        source_name, temp_file_str
    );
    let stop_signal = Arc::new(std::sync::atomic::AtomicBool::new(false));
    super::spawn_file_size_monitor(
        ah.clone(),
        temp_file_str.clone(),
        source_name.to_string(),
        "operation.moving_wsl1_step1".into(),
        stop_signal.clone(),
        None,
    );

    // Yield to event loop before long-running export
    tokio::task::yield_now().await;
    let export_result = {
        let dashboard = {
            let state = as_ptr.lock().await;
            state.wsl_dashboard.clone()
        };
        dashboard.export_distro(source_name, &temp_file_str).await
    };

    stop_signal.store(true, std::sync::atomic::Ordering::Relaxed);

    if !export_result.success {
        let _ = std::fs::remove_file(&temp_file_str);
        return export_result;
    }

    if let Ok(metadata) = std::fs::metadata(&temp_file_str) {
        if metadata.len() == 0 {
            let _ = std::fs::remove_file(&temp_file_str);
            return WslCommandResult::error("".into(), "Exported file is empty".into());
        }
    } else {
        return WslCommandResult::error("".into(), "Failed to verify exported file".into());
    }

    info!("WSL1 Move: Unregistering '{}'...", source_name);
    // Yield before unregister operation
    tokio::task::yield_now().await;
    let unregister_result = {
        let executor = {
            let state = as_ptr.lock().await;
            state.wsl_dashboard.executor().clone()
        };
        executor
            .execute_command(&["--unregister", source_name])
            .await
    };

    if !unregister_result.success {
        let _ = std::fs::remove_file(&temp_file_str);
        return unregister_result;
    }

    info!(
        "WSL1 Move: Importing to '{}' at '{}'...",
        target_name, target_path
    );
    if let Some(app) = ah.upgrade() {
        let msg = i18n::tr("operation.moving_wsl1_step2", &[source_name.to_string()]);
        app.set_task_status_text(msg.into());
    }
    // Yield before long-running import
    tokio::task::yield_now().await;
    let import_result = {
        let dashboard = {
            let state = as_ptr.lock().await;
            state.wsl_dashboard.clone()
        };
        dashboard
            .import_distro(target_name, target_path, &temp_file_str, false)
            .await
    };

    if !import_result.success {
        let _ = std::fs::remove_dir_all(&target_path);
        return WslCommandResult {
            success: false,
            output: "BACKUP_SAVED".into(),
            error: Some(temp_file_str),
            data: None,
            timeout: false,
        };
    }

    // Yield before verification
    tokio::task::yield_now().await;
    let verify_result = {
        let executor = {
            let state = as_ptr.lock().await;
            state.wsl_dashboard.executor().clone()
        };
        executor.execute_command(&["-l", "-v"]).await
    };

    return if verify_result.success && verify_result.output.contains(target_name) {
        info!("WSL1 Move: Success, cleaning up temp file");
        let _ = std::fs::remove_file(&temp_file_str);
        WslCommandResult::success("Move successful".into(), None)
    } else {
        warn!("WSL1 Move: Import appeared successful but distro not found in list");
        WslCommandResult {
            success: false,
            output: "BACKUP_SAVED".into(),
            error: Some(temp_file_str),
            data: None,
            timeout: false,
        }
    };
}

pub async fn perform_batch_move(
    ah: slint::Weak<AppWindow>,
    as_ptr: Arc<Mutex<AppState>>,
    source_name: String,
    target_name: String,
    target_path: String,
    version: String,
    use_elevation: bool,
) -> bool {
    let (result, old_install_location) = do_move_inner(
        ah.clone(),
        as_ptr.clone(),
        &source_name,
        &target_name,
        &target_path,
        &version,
        use_elevation,
    )
    .await;

    if result.success {
        if let Some(ref src_path) = old_install_location {
            let ico_src = std::path::Path::new(src_path).join("shortcut.ico");
            if ico_src.exists() {
                let ico_dst = std::path::Path::new(&target_path).join("shortcut.ico");
                if ico_src != ico_dst {
                    info!("Moving shortcut.ico from {:?} to {:?}", ico_src, ico_dst);
                    let _ = std::fs::copy(&ico_src, &ico_dst);
                }
            }
            let _ = std::fs::remove_dir_all(src_path);
        }
        refresh_distros_ui(ah.clone(), as_ptr.clone()).await;
        true
    } else {
        false
    }
}
