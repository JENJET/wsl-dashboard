use crate::ui::data::refresh_distros_ui;
use crate::wsl::dashboard::operation_guard::DistroOpGuard;
use crate::wsl::models::WslCommandResult;
use crate::{AppState, AppWindow, i18n};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

enum CloneResult {
    Success,
    FailedExport(String),
    FailedImport(String),
}

async fn do_clone_inner(
    ah: slint::Weak<AppWindow>,
    as_ptr: Arc<Mutex<AppState>>,
    source_name: &str,
    target_name: &str,
    target_path: &str,
) -> CloneResult {
    let (executor, dashboard) = {
        let state = as_ptr.lock().await;
        (
            state.wsl_dashboard.executor().clone(),
            state.wsl_dashboard.clone(),
        )
    };
    let _clone_guard = DistroOpGuard::create(
        dashboard.clone(),
        source_name.to_string(),
        "operation.cloning".into(),
    )
    .await;
    let distro_info = crate::wsl::ops::info::get_distro_information(&executor, source_name).await;
    let old_install_location = executor.get_distro_install_location(source_name).await.data;

    let is_wsl2 = distro_info.success
        && distro_info
            .data
            .as_ref()
            .map_or(false, |info| info.wsl_version.to_uppercase() == "WSL2");
    let vhdx_path = distro_info
        .data
        .as_ref()
        .map(|info| info.vhdx_path.clone())
        .unwrap_or_default();

    dashboard.increment_manual_operation();
    let dashboard_clone = dashboard.clone();
    let _manual_op_guard = scopeguard::guard((), |_| {
        dashboard_clone.decrement_manual_operation();
    });

    if is_wsl2 && !vhdx_path.is_empty() {
        info!(
            "WSL2 Clone: Terminating '{}' to release VHDX lock...",
            source_name
        );
        let _ = executor
            .execute_command(&["--terminate", source_name])
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(3000)).await;
        let stop_signal = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let target_path_buf = std::path::Path::new(target_path);
        for vhd_rel in &["ext4.vhdx", "LocalState/ext4.vhdx"] {
            let vhd_path = target_path_buf.join(vhd_rel);
            let _ = super::spawn_file_size_monitor(
                ah.clone(),
                vhd_path.to_string_lossy().to_string(),
                source_name.to_string(),
                "operation.cloning_step2".into(),
                stop_signal.clone(),
                None,
            );
        }
        let _ = std::fs::create_dir_all(target_path_buf);

        let mut import_result = WslCommandResult::error("".to_string(), "".to_string());
        for _ in 0..=5 {
            import_result = executor
                .import_distro(target_name, target_path, &vhdx_path, true)
                .await;
            if import_result.output.contains("ERROR_SHARING_VIOLATION")
                || import_result.output.contains("0x80070020")
            {
                tokio::time::sleep(std::time::Duration::from_millis(3000)).await;
                continue;
            }
            break;
        }
        stop_signal.store(true, std::sync::atomic::Ordering::Relaxed);

        if import_result.success {
            if let Some(ref src_path) = old_install_location {
                let ico_src = std::path::Path::new(src_path).join("shortcut.ico");
                if ico_src.exists() {
                    let ico_dst = std::path::Path::new(target_path).join("shortcut.ico");
                    let _ = std::fs::copy(&ico_src, &ico_dst);
                }
            }
            refresh_distros_ui(ah.clone(), as_ptr.clone()).await;
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            CloneResult::Success
        } else {
            let err = import_result
                .error
                .unwrap_or_else(|| i18n::t("dialog.error"));
            let _ = std::fs::remove_dir_all(target_path);
            CloneResult::FailedImport(err)
        }
    } else {
        //wsl1 or fallback clone: export to tar and import back
        let (temp_dir, temp_file_str) =
            super::resolve_temp_path(as_ptr.clone(), source_name, "wsl_clone", "tar").await;
        let _ = std::fs::create_dir_all(&temp_dir);

        let stop_signal = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let _ = super::spawn_file_size_monitor(
            ah.clone(),
            temp_file_str.clone(),
            source_name.to_string(),
            "operation.cloning_step1".into(),
            stop_signal.clone(),
            None,
        );

        tokio::task::yield_now().await;
        let export_result = {
            info!(
                "WSL1/Fallback Clone: exporting source '{}' to temp file '{}'...",
                source_name, temp_file_str
            );
            let result = executor.export_distro(source_name, &temp_file_str).await;
            result
        };
        stop_signal.store(true, std::sync::atomic::Ordering::Relaxed);

        if !export_result.success {
            let err = export_result.error.unwrap_or_default();
            let _ = std::fs::remove_file(&temp_file_str);
            return CloneResult::FailedExport(err);
        }

        let ah2 = ah.clone();
        let target_name2 = target_name.to_string();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = ah2.upgrade() {
                let msg = i18n::tr("operation.cloning_step2_wsl1", &[target_name2]);
                app.set_task_status_text(msg.into());
                app.set_task_status_visible(true);
            }
        });

        tokio::task::yield_now().await;
        let import_result = {
            info!(
                "WSL1/Fallback Clone: importing as '{}' to '{}'...",
                target_name, target_path
            );
            let result = executor
                .import_distro(target_name, target_path, &temp_file_str, false)
                .await;
            result
        };
        let _ = std::fs::remove_file(&temp_file_str);

        if import_result.success {
            if let Some(ref src_path) = old_install_location {
                let ico_src = std::path::Path::new(src_path).join("shortcut.ico");
                if ico_src.exists() {
                    let ico_dst = std::path::Path::new(target_path).join("shortcut.ico");
                    let _ = std::fs::copy(&ico_src, &ico_dst);
                }
            }
            refresh_distros_ui(ah.clone(), as_ptr.clone()).await;
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            CloneResult::Success
        } else {
            let err = import_result
                .error
                .unwrap_or_else(|| i18n::t("dialog.error"));
            let _ = std::fs::remove_dir_all(target_path);
            CloneResult::FailedImport(err)
        }
    }
}

pub async fn perform_clone(
    ah_clone: slint::Weak<AppWindow>,
    as_ptr: Arc<Mutex<AppState>>,
    source_name: String,
    target_name: String,
    target_path: String,
) {
    let ah_init = ah_clone.clone();
    let name = source_name.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = ah_init.upgrade() {
            app.set_is_cloning(true);
            app.set_task_status_text(
                i18n::tr("operation.cloning_step1", &[name, "0 MB".to_string()]).into(),
            );
            app.set_task_status_visible(true);
        }
    });

    let result = do_clone_inner(
        ah_clone.clone(),
        as_ptr.clone(),
        &source_name,
        &target_name,
        &target_path,
    )
    .await;

    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = ah_clone.upgrade() {
            app.set_task_status_visible(false);
            app.set_is_cloning(false);
            match result {
                CloneResult::Success => {
                    app.set_current_message(
                        i18n::tr("dialog.clone_success", &[source_name, target_name]).into(),
                    );
                }
                CloneResult::FailedExport(err) => {
                    app.set_current_message(i18n::tr("dialog.clone_failed_export", &[err]).into());
                }
                CloneResult::FailedImport(err) => {
                    app.set_current_message(i18n::tr("dialog.clone_failed_import", &[err]).into());
                }
            }
            app.set_show_message_dialog(true);
        }
    });
}

pub async fn perform_batch_clone(
    ah: slint::Weak<AppWindow>,
    as_ptr: Arc<Mutex<AppState>>,
    source_name: String,
    target_name: String,
    target_path: String,
) -> bool {
    let result = do_clone_inner(
        ah.clone(),
        as_ptr.clone(),
        &source_name,
        &target_name,
        &target_path,
    )
    .await;

    matches!(result, CloneResult::Success)
}
