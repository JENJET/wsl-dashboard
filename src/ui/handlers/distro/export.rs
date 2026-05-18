use crate::{AppState, AppWindow, i18n};
use slint::ComponentHandle;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

pub fn setup(app: &AppWindow, app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    // Export process
    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_open_export_dialog(move |name| {
            info!("Operation: Open export dialog - {}", name);
            let ah = ah_outer.clone();
            let as_ptr = as_outer.clone();
            let name_str = name.to_string();

            tokio::spawn(async move {
                let manager = {
                    let state = as_ptr.lock().await;
                    state.wsl_dashboard.clone()
                };

                // Sentinel Check: Distro busy?
                if let Some(op) = manager.get_active_op(&name_str) {
                    let msg = i18n::tr("toast.distro_busy", &[name_str.clone(), op.to_string()]);
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

                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah.upgrade() {
                        if app.get_is_exporting() || app.get_is_cloning() || app.get_is_moving() {
                            app.set_current_message(i18n::t("dialog.operation_in_progress").into());
                            app.set_show_message_dialog(true);
                            return;
                        }
                        app.set_export_distro_name(name_str.into());
                        app.set_export_compress(true);
                        let default_path = app.get_distro_location();
                        app.set_export_target_path(default_path);
                        app.set_export_error("".into());
                        app.set_show_export_dialog(true);
                    }
                });
            });
        });
    }

    {
        let ah_select = app_handle.clone();
        app.on_select_export_folder(move || {
            if let Some(path) = rfd::FileDialog::new()
                .set_title(i18n::t("dialog.select_export_dir"))
                .pick_folder()
            {
                if let Some(app) = ah_select.upgrade() {
                    app.set_export_target_path(path.display().to_string().into());
                }
            }
        });
    }

    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_confirm_export(move |distro_source, target_path| {
            info!(
                "Operation: Confirm export - Source: {}, Target Dir: {}",
                distro_source, target_path
            );

            let ah_weak = ah_outer.clone();
            let as_ptr_outer = as_outer.clone();
            let distro_source = distro_source.to_string();
            let target_path = target_path.to_string();

            let _ = slint::spawn_local(async move {
                // Sentinel Check: System heavy op?
                let manager = {
                    let state = as_ptr_outer.lock().await;
                    state.wsl_dashboard.clone()
                };

                if manager.heavy_op_lock().try_lock().is_err() {
                    let msg = i18n::t("toast.system_busy");
                    if let Some(app) = ah_weak.upgrade() {
                        app.set_current_message(msg.into());
                        app.set_show_message_dialog(true);
                    }
                    return;
                }

                if let Some(app) = ah_weak.upgrade() {
                    let app: AppWindow = app;
                    if target_path.is_empty() {
                        app.set_export_error(i18n::t("dialog.select_target_dir").into());
                        return;
                    }

                    if app.get_is_exporting() || app.get_is_cloning() || app.get_is_moving() {
                        return;
                    }

                    app.set_export_error("".into());
                    let use_compress_inner = app.get_export_compress();
                    app.set_show_export_dialog(false);

                    app.set_is_exporting(true);

                    let ah_clone = app.as_weak();
                    let as_ptr = as_ptr_outer.clone();
                    let distro_source_inner = distro_source.to_string();
                    let target_path_inner = target_path.to_string();

                    let _ = slint::spawn_local(async move {
                        let (success, error_msg, file_path) = do_export(
                            ah_clone.clone(),
                            as_ptr,
                            &distro_source_inner,
                            &target_path_inner,
                            use_compress_inner,
                        )
                        .await;

                        if let Some(app) = ah_clone.upgrade() {
                            app.set_task_status_visible(false);
                            app.set_is_exporting(false);

                            if success {
                                app.set_current_message(
                                    i18n::tr(
                                        "dialog.export_success",
                                        &[distro_source_inner.clone(), file_path.clone()],
                                    )
                                    .into(),
                                );
                            } else {
                                let err = error_msg.unwrap_or_else(|| i18n::t("dialog.error"));
                                app.set_current_message(
                                    i18n::tr("dialog.export_failed", &[err]).into(),
                                );
                            }
                            app.set_show_message_dialog(true);
                        }
                    });
                }
            });
        });
    }
}

/// Core export logic: generate file path, run export, clean up on failure.
/// Returns (success, error_message, final_file_path).
pub async fn do_export(
    ah: slint::Weak<AppWindow>,
    as_ptr: Arc<Mutex<AppState>>,
    distro_name: &str,
    target_dir: &str,
    compress: bool,
) -> (bool, Option<String>, String) {
    let stop_signal = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Generate file path (handle duplicates with timestamp)
    let extension = if compress { "tar.gz" } else { "tar" };
    let mut filename = format!("{}.{}", distro_name, extension);
    let mut export_file = std::path::Path::new(target_dir).join(&filename);

    if export_file.exists() {
        let timestamp = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
        filename = format!("{}.{}.{}", distro_name, timestamp, extension);
        export_file = std::path::Path::new(target_dir).join(&filename);
    }

    let export_file_str = export_file.to_string_lossy().to_string();

    // Start file size monitor
    super::spawn_file_size_monitor(
        ah.clone(),
        export_file_str.clone(),
        distro_name.to_string(),
        "operation.exporting_msg".into(),
        stop_signal.clone(),
        None,
    );

    tokio::task::yield_now().await;

    // Execute export
    let dashboard = {
        let state = as_ptr.lock().await;
        state.wsl_dashboard.clone()
    };

    info!(
        "Exporting distribution '{}' to '{}'...",
        distro_name, export_file_str
    );
    let result = dashboard.export_distro(distro_name, &export_file_str).await;
    stop_signal.store(true, std::sync::atomic::Ordering::Relaxed);

    if result.success {
        (true, None, export_file_str)
    } else {
        let err = result.error.unwrap_or_else(|| i18n::t("dialog.error"));
        let _ = std::fs::remove_file(&export_file_str);
        warn!("Export failed for '{}': {}", distro_name, err);
        (false, Some(err), export_file_str)
    }
}
