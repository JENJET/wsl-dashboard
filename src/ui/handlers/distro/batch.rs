use crate::network::tracker;
use crate::{AppState, AppWindow, i18n};
use scopeguard;
use slint::Model;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;
use tracing::{info, warn};

static BATCH_CANCEL: AtomicBool = AtomicBool::new(false);

pub fn setup(app: &AppWindow, app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    // Toggle batch mode on/off
    {
        let ah = app_handle.clone();
        let as_ptr = app_state.clone();
        app.on_toggle_batch_mode(move || {
            let ah = ah.clone();
            let as_ptr = as_ptr.clone();
            if let Some(app) = ah.upgrade() {
                let new_mode = !app.get_batch_mode();
                app.set_batch_mode(new_mode);
                if new_mode {
                    app.set_batch_abort_triggered(false);
                }

                let _ = slint::spawn_local({
                    let ah = ah.clone();
                    let as_ptr = as_ptr.clone();
                    async move {
                        let mut state = as_ptr.lock().await;
                        state.selected_distros.clear();
                        drop(state);

                        if let Some(app) = ah.upgrade() {
                            if new_mode {
                                app.set_expanded_distro("".into());
                            }
                            clear_model_selection(&app);
                            app.set_selected_count(0);
                            app.set_batch_operating(false);
                            tracker::BATCH_OPERATING.store(false, Ordering::SeqCst);
                        }
                    }
                });
            }
        });
    }

    // Toggle selection for a distro
    {
        let ah = app_handle.clone();
        let as_ptr = app_state.clone();
        app.on_distro_selection_toggled(move |name| {
            let ah = ah.clone();
            let as_ptr = as_ptr.clone();
            let name = name.to_string();
            tokio::spawn(async move {
                let mut state = as_ptr.lock().await;
                let selected = !state.selected_distros.contains(&name);
                if selected {
                    state.selected_distros.insert(name.clone());
                } else {
                    state.selected_distros.remove(&name);
                }
                let count = state.selected_distros.len() as i32;
                drop(state);

                let _ = slint::invoke_from_event_loop({
                    let ah = ah.clone();
                    let name = name.clone();
                    move || {
                        if let Some(app) = ah.upgrade() {
                            update_model_selection(&app, &name, selected);
                            app.set_selected_count(count);
                        }
                    }
                });
            });
        });
    }

    // Batch select all
    {
        let ah = app_handle.clone();
        let as_ptr = app_state.clone();
        app.on_batch_select_all(move || {
            let ah = ah.clone();
            let as_ptr = as_ptr.clone();
            if let Some(app) = ah.upgrade() {
                let names: Vec<String> = (0..app.get_distros().row_count())
                    .filter_map(|i| app.get_distros().row_data(i))
                    .map(|d| d.name.to_string())
                    .collect();

                tokio::spawn(async move {
                    let mut state = as_ptr.lock().await;
                    for name in &names {
                        state.selected_distros.insert(name.clone());
                    }
                    let count = state.selected_distros.len() as i32;
                    drop(state);

                    let ah = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah.upgrade() {
                            for name in &names {
                                update_model_selection(&app, name, true);
                            }
                            app.set_selected_count(count);
                        }
                    });
                });
            }
        });
    }

    // Batch invert selection
    {
        let ah = app_handle.clone();
        let as_ptr = app_state.clone();
        app.on_batch_invert_selection(move || {
            let ah = ah.clone();
            let as_ptr = as_ptr.clone();
            if let Some(app) = ah.upgrade() {
                let states: Vec<(String, bool)> = (0..app.get_distros().row_count())
                    .filter_map(|i| app.get_distros().row_data(i))
                    .map(|d| (d.name.to_string(), d.selected))
                    .collect();

                tokio::spawn(async move {
                    let mut state = as_ptr.lock().await;
                    for (name, was_selected) in &states {
                        if *was_selected {
                            state.selected_distros.remove(name);
                        } else {
                            state.selected_distros.insert(name.clone());
                        }
                    }
                    let count = state.selected_distros.len() as i32;
                    drop(state);

                    let ah = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah.upgrade() {
                            for (name, was_selected) in &states {
                                update_model_selection(&app, name, !was_selected);
                            }
                            app.set_selected_count(count);
                        }
                    });
                });
            }
        });
    }

    // Batch select running
    {
        let ah = app_handle.clone();
        let as_ptr = app_state.clone();
        app.on_batch_select_running(move || {
            let ah = ah.clone();
            let as_ptr = as_ptr.clone();
            if let Some(app) = ah.upgrade() {
                let names: Vec<String> = (0..app.get_distros().row_count())
                    .filter_map(|i| app.get_distros().row_data(i))
                    .filter(|d| d.status == "Running")
                    .map(|d| d.name.to_string())
                    .collect();

                tokio::spawn(async move {
                    let mut state = as_ptr.lock().await;
                    for name in &names {
                        state.selected_distros.insert(name.clone());
                    }
                    let count = state.selected_distros.len() as i32;
                    drop(state);

                    let ah = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah.upgrade() {
                            for name in &names {
                                update_model_selection(&app, name, true);
                            }
                            app.set_selected_count(count);
                        }
                    });
                });
            }
        });
    }

    // Batch select stopped
    {
        let ah = app_handle.clone();
        let as_ptr = app_state.clone();
        app.on_batch_select_stopped(move || {
            let ah = ah.clone();
            let as_ptr = as_ptr.clone();
            if let Some(app) = ah.upgrade() {
                let names: Vec<String> = (0..app.get_distros().row_count())
                    .filter_map(|i| app.get_distros().row_data(i))
                    .filter(|d| d.status == "Stopped")
                    .map(|d| d.name.to_string())
                    .collect();

                tokio::spawn(async move {
                    let mut state = as_ptr.lock().await;
                    for name in &names {
                        state.selected_distros.insert(name.clone());
                    }
                    let count = state.selected_distros.len() as i32;
                    drop(state);

                    let ah = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah.upgrade() {
                            for name in &names {
                                update_model_selection(&app, name, true);
                            }
                            app.set_selected_count(count);
                        }
                    });
                });
            }
        });
    }

    // Batch cancel
    {
        app.on_batch_cancel(move || {
            BATCH_CANCEL.store(true, Ordering::SeqCst);
        });
    }

    // Batch start
    {
        let ah = app_handle.clone();
        let as_ptr = app_state.clone();
        app.on_batch_start(move || {
            let ah = ah.clone();
            let as_ptr = as_ptr.clone();
            tokio::spawn(async move {
                let names = get_selected_names(&as_ptr).await;
                if names.is_empty() {
                    return;
                }
                info!("Batch start: {:?}", names);
                run_batch_op(&ah, &as_ptr, names, "operation.starting", |m, n| {
                    Box::pin(async move { m.start_distro(&n).await })
                })
                .await;
            });
        });
    }

    // Batch stop
    {
        let ah = app_handle.clone();
        let as_ptr = app_state.clone();
        app.on_batch_stop(move || {
            let ah = ah.clone();
            let as_ptr = as_ptr.clone();
            tokio::spawn(async move {
                let names = get_selected_names(&as_ptr).await;
                if names.is_empty() {
                    return;
                }
                info!("Batch stop: {:?}", names);
                run_batch_op(&ah, &as_ptr, names, "operation.stopping", |m, n| {
                    Box::pin(async move { m.stop_distro(&n).await })
                })
                .await;
            });
        });
    }

    // Batch restart
    {
        let ah = app_handle.clone();
        let as_ptr = app_state.clone();
        app.on_batch_restart(move || {
            let ah = ah.clone();
            let as_ptr = as_ptr.clone();
            tokio::spawn(async move {
                let names = get_selected_names(&as_ptr).await;
                if names.is_empty() {
                    return;
                }
                info!("Batch restart: {:?}", names);
                run_batch_op(&ah, &as_ptr, names, "operation.restarting", |m, n| {
                    Box::pin(async move { m.restart_distro(&n).await })
                })
                .await;
            });
        });
    }

    // Batch export - show dialog first
    {
        let ah = app_handle.clone();
        app.on_batch_export(move || {
            let ah = ah.clone();
            if let Some(app) = ah.upgrade() {
                app.set_export_distro_name("".into());
                app.set_export_compress(true);
                let default_path = app.get_distro_location();
                app.set_export_target_path(default_path);
                app.set_export_error("".into());
                app.set_show_export_dialog(true);
            }
        });
    }

    // Batch confirm export - actually run the export
    {
        let ah = app_handle.clone();
        let as_ptr = app_state.clone();
        app.on_batch_confirm_export(move |target_path| {
            let ah = ah.clone();
            let as_ptr = as_ptr.clone();
            let target_path = target_path.to_string();
            tokio::spawn(async move {
                let names = get_selected_names(&as_ptr).await;
                if names.is_empty() {
                    return;
                }

                // Close dialog and set operating immediately
                let _ = slint::invoke_from_event_loop({
                    let ah = ah.clone();
                    move || {
                        if let Some(app) = ah.upgrade() {
                            app.set_show_export_dialog(false);
                            app.set_batch_operating(true);
                            tracker::BATCH_OPERATING.store(true, Ordering::SeqCst);
                            app.set_is_exporting(true);
                        }
                    }
                });

                let use_compress = {
                    let ah = ah.clone();
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah.upgrade() {
                            let _ = tx.send(app.get_export_compress());
                        }
                    });
                    rx.await.unwrap_or(true)
                };

                BATCH_CANCEL.store(false, Ordering::SeqCst);
                info!("Batch export: {:?} -> {}", names, target_path);

                let total = names.len();
                let mut cancelled = false;
                let mut failed_count = 0;
                let mut success_count = 0;
                for (i, name) in names.iter().enumerate() {
                    if BATCH_CANCEL.load(Ordering::SeqCst) {
                        info!("Batch export cancelled by user");
                        cancelled = true;
                        break;
                    }

                    // Set progress suffix — do_export will handle the text
                    let _ = slint::invoke_from_event_loop({
                        let ah = ah.clone();
                        let suffix = format!(" [{}/{}]", i + 1, total);
                        move || {
                            if let Some(app) = ah.upgrade() {
                                app.set_batch_progress_suffix(suffix.into());
                                app.set_task_status_visible(true);
                            }
                        }
                    });

                    let (success, _error, _file) = super::export::do_export(
                        ah.clone(),
                        as_ptr.clone(),
                        name,
                        &target_path,
                        use_compress,
                    )
                    .await;

                    if success {
                        success_count += 1;
                    } else {
                        failed_count += 1;
                    }

                    if BATCH_CANCEL.load(Ordering::SeqCst) {
                        cancelled = true;
                        break;
                    }
                }

                let skipped = total as i32 - success_count - failed_count;
                finish_batch(&ah, success_count, failed_count, skipped, cancelled);
                let _ = slint::invoke_from_event_loop({
                    let ah = ah.clone();
                    move || {
                        if let Some(app) = ah.upgrade() {
                            app.set_is_exporting(false);
                        }
                    }
                });
            });
        });
    }

    // Batch clone
    {
        let ah = app_handle.clone();
        app.on_batch_clone(move || {
            let ah = ah.clone();
            if let Some(app) = ah.upgrade() {
                let default_path = app.get_distro_location();
                app.set_clone_source_name("".into());
                app.set_clone_target_name("".into());
                app.set_clone_target_path(default_path);
                app.set_clone_error("".into());
                app.set_show_clone_dialog(true);
            }
        });
    }

    // Batch confirm clone
    {
        let ah = app_handle.clone();
        let as_ptr = app_state.clone();
        app.on_batch_confirm_clone(move |suffix, path| {
            let ah = ah.clone();
            let as_ptr = as_ptr.clone();
            let suffix = suffix.to_string();
            let path = path.to_string();
            let root_path = std::path::PathBuf::from(&path);

            // Validate synchronously before closing dialog
            if let Some(app) = ah.upgrade() {
                // Collect existing distro names
                let existing_names: Vec<String> = {
                    let mut names = Vec::new();
                    let distros = app.get_distros();
                    for j in 0..distros.row_count() {
                        if let Some(d) = distros.row_data(j) {
                            names.push(d.name.to_string());
                        }
                    }
                    names
                };

                let mut target_names: Vec<String> = Vec::new();
                let distros = app.get_distros();
                for j in 0..distros.row_count() {
                    if let Some(d) = distros.row_data(j) {
                        if !d.selected {
                            continue;
                        }
                        let name = d.name.to_string();
                        let target_name = if suffix.is_empty() {
                            super::generate_random_suffix(&name)
                        } else {
                            format!("{}{}", name, suffix)
                        };

                        // Validate characters
                        let is_valid = target_name
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
                        if !is_valid || target_name.len() > crate::app::MAX_INSTANCE_NAME_LEN {
                            app.set_clone_error(i18n::t("dialog.install_name_invalid").into());
                            return;
                        }

                        // Check collision
                        if existing_names.contains(&target_name)
                            || target_names.contains(&target_name)
                        {
                            app.set_clone_error(
                                format!("{}: {}", i18n::t("dialog.name_exists"), target_name)
                                    .into(),
                            );
                            return;
                        }

                        // Check length
                        if target_name.len() > crate::app::MAX_INSTANCE_NAME_LEN {
                            app.set_clone_error(i18n::t("dialog.install_name_invalid").into());
                            return;
                        }

                        // Check path existence (allow empty directories)
                        let target_path = root_path.join(&target_name);
                        if target_path.exists()
                            && (target_path.is_file()
                                || (target_path.is_dir()
                                    && target_path
                                        .read_dir()
                                        .map(|mut d| d.next().is_some())
                                        .unwrap_or(false)))
                        {
                            app.set_clone_error(
                                i18n::tr("dialog.path_exists", &[target_name]).into(),
                            );
                            return;
                        }

                        target_names.push(target_name);
                    }
                }

                let root_path_clone = root_path.clone();
                let target_names_clone = target_names.clone();
                let _ = slint::spawn_local(async move {
                    let dashboard = {
                        let state = as_ptr.lock().await;
                        state.wsl_dashboard.clone()
                    };

                    // Check overlap for each selected distro
                    let app_ui = ah.upgrade();
                    if let Some(app) = app_ui {
                        let mut overlap = false;
                        let distros = app.get_distros();
                        let mut idx = 0;
                        for j in 0..distros.row_count() {
                            if let Some(d) = distros.row_data(j) {
                                if d.selected {
                                    let tname = &target_names_clone[idx];
                                    idx += 1;
                                    let old = dashboard
                                        .executor()
                                        .get_distro_install_location(&d.name)
                                        .await
                                        .data;
                                    let target =
                                        root_path_clone.join(tname).to_string_lossy().to_string();
                                    if let Some(ref old_path) = old {
                                        if super::paths_overlap(old_path, &target) {
                                            app.set_clone_error(
                                                format!(
                                                    "'{}': {}",
                                                    d.name,
                                                    i18n::tr(
                                                        "dialog.path_overlap",
                                                        &[old_path.clone()]
                                                    )
                                                )
                                                .into(),
                                            );
                                            overlap = true;
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        if overlap {
                            return;
                        }
                    }

                    // Close dialog and start batch
                    let total = target_names_clone.len();
                    if let Some(app) = ah.upgrade() {
                        app.set_show_clone_dialog(false);
                        app.set_batch_operating(true);
                        tracker::BATCH_OPERATING.store(true, Ordering::SeqCst);
                        app.set_is_cloning(true);
                        app.set_task_status_text(i18n::t("operation.cloning").into());
                        app.set_batch_progress_suffix(format!(" [0/{}]", total).into());
                        app.set_task_status_visible(true);
                    }

                    BATCH_CANCEL.store(false, Ordering::SeqCst);

                    tokio::spawn(async move {
                        let names = get_selected_names(&as_ptr).await;
                        if names.is_empty() {
                            return;
                        }

                        let mut failed_count = 0;
                        let mut success_count = 0;
                        let mut cancelled = false;

                        for (i, name) in names.iter().enumerate() {
                            if BATCH_CANCEL.load(Ordering::SeqCst) {
                                info!("Batch clone cancelled by user");
                                cancelled = true;
                                break;
                            }

                            let target_name = &target_names[i];

                            info!(
                                "Batch clone processing [{}/{}]: {} -> {}",
                                i + 1,
                                total,
                                name,
                                target_name
                            );

                            let _ = slint::invoke_from_event_loop({
                                let ah = ah.clone();
                                let name = name.clone();
                                let suffix = format!(" [{}/{}]", i + 1, total);
                                move || {
                                    if let Some(app) = ah.upgrade() {
                                        app.set_task_status_text(
                                            format!("{} {}", name, i18n::t("operation.cloning"))
                                                .into(),
                                        );
                                        app.set_batch_progress_suffix(suffix.into());
                                        app.set_task_status_visible(true);
                                    }
                                }
                            });

                            let target_path =
                                root_path.join(target_name).to_string_lossy().to_string();

                            let name_clone = name.clone();
                            let tname = target_name.clone();
                            let success = super::clone_logic::perform_batch_clone(
                                ah.clone(),
                                as_ptr.clone(),
                                name_clone,
                                tname,
                                target_path,
                            )
                            .await;

                            info!(
                                "Batch clone [{}/{}] {} -> {}: success={}",
                                i + 1,
                                total,
                                name,
                                target_name,
                                success
                            );
                            if success {
                                success_count += 1;
                            } else {
                                failed_count += 1;
                            }
                        }

                        let skipped = total as i32 - success_count - failed_count;
                        finish_batch(&ah, success_count, failed_count, skipped, cancelled);
                        let _ = slint::invoke_from_event_loop({
                            let ah = ah.clone();
                            move || {
                                if let Some(app) = ah.upgrade() {
                                    app.set_is_cloning(false);
                                }
                            }
                        });
                    });
                });
            }
        });
    }

    // Batch move
    {
        let ah = app_handle.clone();
        app.on_batch_move(move || {
            let ah = ah.clone();
            if let Some(app) = ah.upgrade() {
                let default_path = app.get_distro_location();
                app.set_move_source_name("".into());
                app.set_move_target_name("".into());
                app.set_move_target_path(default_path);
                app.set_move_error("".into());
                app.set_show_move_dialog(true);
            }
        });
    }

    // Batch confirm move
    {
        let ah = app_handle.clone();
        let as_ptr = app_state.clone();
        app.on_batch_confirm_move(move |path| {
            let ah = ah.clone();
            let as_ptr = as_ptr.clone();
            let path = path.to_string();
            let root_path = std::path::PathBuf::from(&path);

            // Validate synchronously before closing dialog
            let app_ui = ah.upgrade();
            if let Some(app) = app_ui {
                // Access model through the app handle
                let mut valid = true;
                let distros = app.get_distros();
                for i in 0..distros.row_count() {
                    if let Some(d) = distros.row_data(i) {
                        if d.selected {
                            let target_path = root_path.join(d.name.as_str());
                            if target_path.exists()
                                && (target_path.is_file()
                                    || (target_path.is_dir()
                                        && target_path
                                            .read_dir()
                                            .map(|mut d| d.next().is_some())
                                            .unwrap_or(false)))
                            {
                                app.set_move_error(
                                    i18n::tr("dialog.path_exists", &[d.name.to_string()]).into(),
                                );
                                valid = false;
                                break;
                            }
                        }
                    }
                }
                if !valid {
                    return;
                }

                let _ = slint::spawn_local(async move {
                    let dashboard = {
                        let state = as_ptr.lock().await;
                        state.wsl_dashboard.clone()
                    };

                    // Check overlap for each selected distro
                    let app_ui = ah.upgrade();
                    if let Some(app) = app_ui {
                        let mut overlap = false;
                        let distros = app.get_distros();
                        for i in 0..distros.row_count() {
                            if let Some(d) = distros.row_data(i) {
                                if d.selected {
                                    let old = dashboard
                                        .executor()
                                        .get_distro_install_location(&d.name)
                                        .await
                                        .data;
                                    let target = root_path
                                        .join(d.name.as_str())
                                        .to_string_lossy()
                                        .to_string();
                                    if let Some(ref old_path) = old {
                                        if super::paths_overlap(old_path, &target) {
                                            app.set_move_error(
                                                format!(
                                                    "'{}': {}",
                                                    d.name,
                                                    i18n::tr(
                                                        "dialog.path_overlap",
                                                        &[old_path.clone()]
                                                    )
                                                )
                                                .into(),
                                            );
                                            overlap = true;
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        if overlap {
                            return;
                        }
                    }

                    // Close dialog and start batch
                    if let Some(app) = ah.upgrade() {
                        app.set_show_move_dialog(false);
                        app.set_batch_operating(true);
                        tracker::BATCH_OPERATING.store(true, Ordering::SeqCst);
                        app.set_is_moving(true);
                        app.set_task_status_text(i18n::t("operation.moving").into());
                        app.set_task_status_visible(true);
                    }

                    tokio::spawn(async move {
                        let root_path = std::path::PathBuf::from(&path);

                        let names = get_selected_names(&as_ptr).await;
                        if names.is_empty() {
                            return;
                        }

                        let names_len = names.len();

                        // Set initial progress suffix with actual total
                        let _ = slint::invoke_from_event_loop({
                            let ah = ah.clone();
                            let suffix = format!(" [0/{}]", names_len);
                            move || {
                                if let Some(app) = ah.upgrade() {
                                    app.set_task_status_text(i18n::t("operation.moving").into());
                                    app.set_batch_progress_suffix(suffix.into());
                                    app.set_task_status_visible(true);
                                }
                            }
                        });

                        BATCH_CANCEL.store(false, Ordering::SeqCst);
                        let total = names_len;
                        let mut failed_count = 0;
                        let mut success_count = 0;
                        let mut cancelled = false;

                        for (i, name) in names.iter().enumerate() {
                            if BATCH_CANCEL.load(Ordering::SeqCst) {
                                info!("Batch move cancelled by user");
                                cancelled = true;
                                break;
                            }

                            let _ = slint::invoke_from_event_loop({
                                let ah = ah.clone();
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
                                let app_ui = ah.upgrade();
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
                                ah.clone(),
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
                            } else {
                                failed_count += 1;
                            }
                        }

                        let skipped = total as i32 - success_count - failed_count;
                        finish_batch(&ah, success_count, failed_count, skipped, cancelled);
                        let _ = slint::invoke_from_event_loop({
                            let ah = ah.clone();
                            move || {
                                if let Some(app) = ah.upgrade() {
                                    app.set_is_moving(false);
                                }
                            }
                        });
                    });
                });
            }
        });
    }

    // Batch delete
    {
        let ah = app_handle.clone();
        let as_ptr = app_state.clone();
        app.on_batch_delete(move || {
            let ah = ah.clone();
            let as_ptr = as_ptr.clone();
            tokio::spawn(async move {
                let names = get_selected_names(&as_ptr).await;
                if names.is_empty() {
                    return;
                }
                info!("Batch delete: {:?}", names);
                run_batch_op(&ah, &as_ptr, names, "operation.deleting", {
                    let as_ptr = as_ptr.clone();
                    move |m, n| {
                        let as_ptr = as_ptr.clone();
                        Box::pin(async move {
                            let config_manager = {
                                let state = as_ptr.lock().await;
                                state.config_manager.clone()
                            };

                            if m.get_active_op(&n).is_some() {
                                return crate::wsl::models::WslCommandResult::error(
                                    String::new(),
                                    format!("'{}' is busy", n),
                                );
                            }

                            if m.heavy_op_lock().try_lock().is_err() {
                                return crate::wsl::models::WslCommandResult::error(
                                    String::new(),
                                    "System busy".into(),
                                );
                            }

                            m.delete_distro(&config_manager, &n).await
                        })
                    }
                })
                .await;
            });
        });
    }
}

fn finish_batch(
    ah: &slint::Weak<AppWindow>,
    success_count: i32,
    failed_count: i32,
    skipped_count: i32,
    cancelled: bool,
) {
    let ah = ah.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = ah.upgrade() {
            app.set_task_status_visible(false);
            app.set_batch_operating(false);
            tracker::BATCH_OPERATING.store(false, Ordering::SeqCst);
            app.set_batch_mode(false);
            app.set_selected_count(0);
            app.set_batch_progress_suffix("".into());
            clear_model_selection(&app);
            if cancelled {
                app.set_current_message(
                    i18n::tr(
                        "batch.cancelled",
                        &[
                            success_count.to_string(),
                            failed_count.to_string(),
                            skipped_count.to_string(),
                        ],
                    )
                    .into(),
                );
            } else {
                app.set_current_message(
                    i18n::tr(
                        "batch.completed",
                        &[
                            success_count.to_string(),
                            failed_count.to_string(),
                            skipped_count.to_string(),
                        ],
                    )
                    .into(),
                );
            }
            app.set_show_message_dialog(true);
        }
    });
}

fn clear_model_selection(app: &AppWindow) {
    let model = app.get_distros();
    for i in 0..model.row_count() {
        if let Some(mut distro) = model.row_data(i) {
            if distro.selected {
                distro.selected = false;
                model.set_row_data(i, distro);
            }
        }
    }
}

fn update_model_selection(app: &AppWindow, name: &str, selected: bool) {
    let model = app.get_distros();
    for i in 0..model.row_count() {
        if let Some(mut distro) = model.row_data(i) {
            if distro.name.as_str() == name {
                distro.selected = selected;
                model.set_row_data(i, distro);
                return;
            }
        }
    }
}

async fn get_selected_names(as_ptr: &Arc<Mutex<AppState>>) -> Vec<String> {
    let state = as_ptr.lock().await;
    state.selected_distros.iter().cloned().collect()
}

async fn run_batch_op<F>(
    ah: &slint::Weak<AppWindow>,
    as_ptr: &Arc<Mutex<AppState>>,
    names: Vec<String>,
    status_key: &str,
    op: F,
) where
    F: Fn(
            crate::wsl::dashboard::WslDashboard,
            String,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = crate::wsl::models::WslCommandResult<String>>
                    + Send,
            >,
        > + Send
        + Sync,
{
    // Set operating flag and show toast before loop
    let _ = slint::invoke_from_event_loop({
        let ah = ah.clone();
        let status_key = status_key.to_string();
        let total = names.len();
        move || {
            if let Some(app) = ah.upgrade() {
                app.set_batch_operating(true);
                tracker::BATCH_OPERATING.store(true, Ordering::SeqCst);
                app.set_task_status_text(i18n::t(&status_key).into());
                app.set_batch_progress_suffix(format!(" [0/{}]", total).into());
                app.set_task_status_visible(true);
            }
        }
    });

    BATCH_CANCEL.store(false, Ordering::SeqCst);
    let total = names.len();
    let mut cancelled = false;
    let mut failed_count = 0;
    let mut success_count = 0;

    // Clone dashboard once before the loop (WslDashboard is cheap: all fields are Arc)
    let manager = {
        let state = as_ptr.lock().await;
        state.wsl_dashboard.clone()
    };

    // Ensure cleanup runs even if op panics
    let ah_cleanup = ah.clone();
    let _cleanup_guard = scopeguard::guard((), move |_| {
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = ah_cleanup.upgrade() {
                app.set_task_status_visible(false);
                app.set_batch_operating(false);
                tracker::BATCH_OPERATING.store(false, Ordering::SeqCst);
                app.set_batch_mode(false);
                app.set_selected_count(0);
                app.set_batch_progress_suffix("".into());
            }
        });
    });

    for (i, name) in names.iter().enumerate() {
        if BATCH_CANCEL.load(Ordering::SeqCst) {
            info!("Batch operation cancelled by user");
            cancelled = true;
            break;
        }

        if manager.get_active_op(name).is_some() {
            let msg = i18n::tr("toast.distro_busy", &[name.clone(), i18n::t(status_key)]);
            let _ = slint::invoke_from_event_loop({
                let ah = ah.clone();
                move || {
                    if let Some(app) = ah.upgrade() {
                        app.set_current_message(msg.into());
                        app.set_show_message_dialog(true);
                    }
                }
            });
            continue;
        }

        let _ = slint::invoke_from_event_loop({
            let ah = ah.clone();
            let suffix = format!(" [{}/{}]", i + 1, total);
            let text = i18n::t(status_key);
            let n = name.clone();
            move || {
                if let Some(app) = ah.upgrade() {
                    app.set_task_status_text(format!("{} {}", n, text).into());
                    app.set_batch_progress_suffix(suffix.into());
                    app.set_task_status_visible(true);
                }
            }
        });

        let result = op(manager.clone(), name.clone()).await;
        if !result.success {
            warn!("Batch operation failed for '{}': {:?}", name, result.error);
            failed_count += 1;
        } else {
            success_count += 1;
        }
    }

    // Normal cleanup — the scopeguard's duplicate will be a no-op
    let ah = ah.clone();
    let skipped_count = total as i32 - success_count - failed_count;
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = ah.upgrade() {
            app.set_task_status_visible(false);
            app.set_batch_operating(false);
            tracker::BATCH_OPERATING.store(false, Ordering::SeqCst);
            app.set_batch_mode(false);
            app.set_selected_count(0);
            app.set_batch_progress_suffix("".into());
            clear_model_selection(&app);
            if cancelled {
                app.set_current_message(
                    i18n::tr(
                        "batch.cancelled",
                        &[
                            success_count.to_string(),
                            failed_count.to_string(),
                            skipped_count.to_string(),
                        ],
                    )
                    .into(),
                );
            } else {
                app.set_current_message(
                    i18n::tr(
                        "batch.completed",
                        &[
                            success_count.to_string(),
                            failed_count.to_string(),
                            skipped_count.to_string(),
                        ],
                    )
                    .into(),
                );
            }
            app.set_show_message_dialog(true);
        }
    });
}
