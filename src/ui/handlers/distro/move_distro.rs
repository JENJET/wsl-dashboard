use crate::network::tracker;
use crate::{AppState, AppWindow, i18n};
use slint::{ComponentHandle, Model};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::Mutex;
use tracing::info;

use super::batch::{BATCH_MOVE_STATE, BatchMoveState};

pub fn setup(app: &AppWindow, app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    // Open Move Dialog
    let ah_open = app_handle.clone();
    let as_open = app_state.clone();
    app.on_open_move_dialog(move |name| {
        info!("Operation: Open move dialog - {}", name);
        let ah = ah_open.clone();
        let as_ptr = as_open.clone();
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
                    if app.get_is_installing()
                        || app.get_is_exporting()
                        || app.get_is_cloning()
                        || app.get_is_moving()
                    {
                        app.set_current_message(i18n::t("dialog.operation_in_progress").into());
                        app.set_show_message_dialog(true);
                        return;
                    }
                    let distro_location = app.get_distro_location().to_string();
                    let target_path = std::path::Path::new(&distro_location)
                        .join(&name_str)
                        .to_string_lossy()
                        .to_string();
                    app.set_move_source_name(name_str.clone().into());
                    app.set_move_target_name(name_str.clone().into());
                    app.set_move_target_path(target_path.into());
                    app.set_move_original_path("".into());
                    app.set_move_error("".into());
                    app.set_show_move_dialog(true);
                }
            });
        });
    });

    let ah_cancel = app_handle.clone();
    app.on_cancel_move_confirm(move || {
        if let Some(app) = ah_cancel.upgrade() {
            app.set_show_move_confirm(false);
            if app.get_move_source_name().is_empty() {
                if let Ok(mut state) = BATCH_MOVE_STATE.lock() {
                    *state = None;
                }
                info!("Operation: Batch move confirm cancelled");
            } else {
                info!("Operation: Move confirm cancelled");
            }
        }
    });

    let ah_confirm = app_handle.clone();
    let as_confirm = app_state.clone();
    app.on_confirm_move_action(move || {
        let ah_weak = ah_confirm.clone();
        let as_ptr = as_confirm.clone();

        let _ = slint::spawn_local(async move {
            // Sentinel Check: System heavy op?
            let manager = {
                let state = as_ptr.lock().await;
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
                app.set_show_move_confirm(false);

                let source_name = app.get_move_source_name().to_string();

                if source_name.is_empty() {
                    // Batch move - read from BATCH_MOVE_STATE
                    info!("Operation: Batch move confirmed");
                    let state = BATCH_MOVE_STATE.lock().ok().and_then(|mut s| s.take());
                    if let Some(BatchMoveState { root_path }) = state {
                        app.set_batch_operating(true);
                        tracker::BATCH_OPERATING.store(true, Ordering::SeqCst);
                        app.set_is_moving(true);
                        app.set_task_status_text(i18n::t("operation.moving").into());
                        app.set_task_status_visible(true);

                        let ah_weak = app.as_weak();
                        let as_ptr = as_ptr.clone();
                        let path = root_path.to_string_lossy().to_string();
                        tokio::spawn(async move {
                            let root_path = std::path::PathBuf::from(&path);
                            let names = super::batch::get_selected_names(&as_ptr).await;
                            if names.is_empty() {
                                return;
                            }
                            let names_len = names.len();
                            let _ = slint::invoke_from_event_loop({
                                let ah = ah_weak.clone();
                                let suffix = format!(" [0/{}]", names_len);
                                move || {
                                    if let Some(app) = ah.upgrade() {
                                        app.set_task_status_text(
                                            i18n::t("operation.moving").into(),
                                        );
                                        app.set_batch_progress_suffix(suffix.into());
                                        app.set_task_status_visible(true);
                                    }
                                }
                            });
                            let names_len = names.len();
                            let total = names_len;
                            let mut failed_count = 0;
                            let mut success_count = 0;
                            for (i, name) in names.iter().enumerate() {
                                let _ = slint::invoke_from_event_loop({
                                    let ah = ah_weak.clone();
                                    let name = name.clone();
                                    let suffix = format!(" [{}/{}]", i + 1, total);
                                    move || {
                                        if let Some(app) = ah.upgrade() {
                                            app.set_task_status_text(
                                                format!("{} {}", name, i18n::t("operation.moving"))
                                                    .into(),
                                            );
                                            app.set_batch_progress_suffix(suffix.into());
                                            app.set_task_status_visible(true);
                                        }
                                    }
                                });
                                let name_clone = name.to_string();
                                let version = {
                                    let app_ui = ah_weak.upgrade();
                                    let mut ver = crate::wsl::models::WslVersion::V2.to_string();
                                    if let Some(app) = app_ui {
                                        let distros = app.get_distros();
                                        for j in 0..distros.row_count() {
                                            if let Some(d) = distros.row_data(j) {
                                                if d.name == name_clone {
                                                    ver = d.version.to_string();
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                    ver
                                };
                                let target_name = name.to_string();
                                let target_path =
                                    root_path.join(&target_name).to_string_lossy().to_string();
                                let success = super::move_logic::perform_batch_move(
                                    ah_weak.clone(),
                                    as_ptr.clone(),
                                    name.to_string(),
                                    target_name,
                                    target_path,
                                    version,
                                    true,
                                )
                                .await;
                                if success {
                                    success_count += 1;
                                    super::batch::deselect_distro(&as_ptr, &ah_weak, name).await;
                                } else {
                                    failed_count += 1;
                                }
                            }
                            let skipped = total as i32 - success_count - failed_count;
                            let cancelled = skipped > 0;
                            super::batch::finish_batch(
                                &ah_weak,
                                success_count,
                                failed_count,
                                skipped,
                                cancelled,
                            );
                            let _ = slint::invoke_from_event_loop({
                                let ah = ah_weak.clone();
                                move || {
                                    if let Some(app) = ah.upgrade() {
                                        app.set_is_moving(false);
                                    }
                                }
                            });
                        });
                    }
                    return;
                }

                let target_name = app.get_move_target_name().to_string();
                let target_path = app.get_move_target_path().to_string();

                info!(
                    "Operation: Move confirmed - Starting WSL2 Move for {}",
                    source_name
                );

                // Synchronously set moving status
                app.set_is_moving(true);

                run_move_process(
                    app.as_weak(),
                    as_ptr.clone(),
                    source_name,
                    target_name,
                    target_path,
                    crate::wsl::models::WslVersion::V2.to_string(),
                    true,
                );
            }
        });
    });

    let ah_folder = app_handle.clone();
    app.on_select_move_folder(move || {
        if let Some(path) = rfd::FileDialog::new()
            .set_title(i18n::t("dialog.select_move_dir"))
            .pick_folder()
        {
            if let Some(app) = ah_folder.upgrade() {
                app.set_move_target_path(path.to_string_lossy().to_string().into());
            }
        }
    });

    let ah_confirm_click = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_confirm_move(move |source_name, _target_name, target_path| {
        info!(
            "Operation: Confirm move - Source: {}, Target: {}, Path: {}",
            source_name, _target_name, target_path
        );

        let ah_weak = ah_confirm_click.clone();
        let as_ptr = as_ptr.clone();
        let source_name = source_name.to_string();
        let target_name = _target_name.to_string();
        let target_path = target_path.to_string();

        let _ = slint::spawn_local(async move {
            let manager = {
                let state = as_ptr.lock().await;
                state.wsl_dashboard.clone()
            };

            // Sentinel Check: Distro busy?
            if let Some(op) = manager.get_active_op(&source_name) {
                let msg = i18n::tr("toast.distro_busy", &[source_name.clone(), op.to_string()]);
                if let Some(app) = ah_weak.upgrade() {
                    app.set_current_message(msg.into());
                    app.set_show_message_dialog(true);
                }
                return;
            }

            // Sentinel Check: System heavy op?
            if manager.heavy_op_lock().try_lock().is_err() {
                let msg = i18n::t("toast.system_busy");
                if let Some(app) = ah_weak.upgrade() {
                    app.set_current_message(msg.into());
                    app.set_show_message_dialog(true);
                }
                return;
            }

            if let Some(app) = ah_weak.upgrade() {
                if app.get_is_installing()
                    || app.get_is_exporting()
                    || app.get_is_cloning()
                    || app.get_is_moving()
                {
                    return;
                }

                // 1. Sync Validations
                let p = std::path::Path::new(target_path.as_str());
                if p.exists() {
                    if p.is_dir() {
                        if let Ok(entries) = std::fs::read_dir(p) {
                            if entries.count() > 0 {
                                app.set_move_error(i18n::t("dialog.dir_not_empty").into());
                                return;
                            }
                        }
                    } else {
                        app.set_move_error(i18n::t("dialog.path_is_not_dir").into());
                        return;
                    }
                }

                // Validation: Target not overlapping with existing install location
                let old_location = manager
                    .executor()
                    .get_distro_install_location(&source_name)
                    .await
                    .data;
                if let Some(ref old) = old_location {
                    if super::paths_overlap(old, &target_path) {
                        app.set_move_error(i18n::tr("dialog.path_overlap", &[old.clone()]).into());
                        return;
                    }
                }

                // Get distro version
                let mut version = crate::wsl::models::WslVersion::V2.to_string();
                let distros = app.get_distros();
                for i in 0..distros.row_count() {
                    if let Some(d) = distros.row_data(i) {
                        if d.name == source_name {
                            version = d.version.to_string();
                            break;
                        }
                    }
                }

                app.set_move_error("".into());

                let is_wsl2 = version == crate::wsl::models::WslVersion::V2.to_string();

                // Collect running distros for warning
                let mut running_names = Vec::new();
                for i in 0..distros.row_count() {
                    if let Some(d) = distros.row_data(i) {
                        if d.status.as_str() == "Running" && (is_wsl2 || d.name == source_name) {
                            running_names.push(d.name.to_string());
                        }
                    }
                }

                if !running_names.is_empty() {
                    let warning_msg =
                        i18n::tr("dialog.move_shutdown_warning", &[running_names.join(", ")]);

                    app.set_move_confirm_message(warning_msg.into());
                    app.set_show_move_confirm(true);
                    app.set_show_move_dialog(false);
                } else {
                    app.set_show_move_dialog(false);
                    app.set_is_moving(true);
                    run_move_process(
                        app.as_weak(),
                        as_ptr.clone(),
                        source_name,
                        target_name,
                        target_path,
                        version,
                        true,
                    );
                }
            }
        });
    });
}

fn run_move_process(
    ah_move: slint::Weak<AppWindow>,
    as_ptr: Arc<Mutex<AppState>>,
    source_name: String,
    target_name: String,
    target_path: String,
    version: String,
    use_elevation: bool,
) {
    super::move_logic::run_move_process(
        ah_move,
        as_ptr,
        source_name,
        target_name,
        target_path,
        version,
        use_elevation,
    );
}
