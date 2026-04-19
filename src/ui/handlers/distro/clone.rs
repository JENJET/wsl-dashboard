use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, error};
use rand::Rng;
use rand::distr::Alphanumeric;
use slint::{ComponentHandle, Model};
use crate::{AppWindow, AppState, i18n};

pub fn setup(app: &AppWindow, app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    // Clone process
    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_open_clone_dialog(move |name| {
            info!("Operation: Open clone dialog - {}", name);
            let ah = ah_outer.clone();
            let as_ptr = as_outer.clone();
            let name_str = name.to_string();

            tokio::spawn(async move {
                let manager = {
                    let state = as_ptr.lock().await;
                    state.wsl_dashboard.clone()
                };

                // Sentinel Check: Distro busy?
                if let Some(op) = manager.get_active_op(&name_str).await {
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
                        if app.get_is_cloning() || app.get_is_exporting() || app.get_is_moving() {
                            app.set_current_message(i18n::t("dialog.operation_in_progress").into());
                            app.set_show_message_dialog(true);
                            return;
                        }
                        // Generate 4-character random alphanumeric string
                        let random_suffix: String = rand::rng()
                            .sample_iter(&Alphanumeric)
                            .take(4)
                            .map(char::from)
                            .collect();
                        
                        let target_name = format!("{}_{}", name_str, random_suffix);
                        let distro_location = app.get_distro_location();
                        let target_path = std::path::Path::new(&distro_location.to_string())
                            .join(&target_name)
                            .to_string_lossy()
                            .to_string();

                        app.set_clone_source_name(name_str.into());
                        app.set_clone_target_name(target_name.into());
                        app.set_clone_target_path(target_path.into());
                        app.set_clone_base_path(distro_location.to_string().into());
                        app.set_clone_error("".into());
                        app.set_show_clone_dialog(true);
                    }
                });
            });
        });
    }

    {
        let ah_select = app_handle.clone();
        app.on_select_clone_folder(move || {
            if let Some(path) = rfd::FileDialog::new()
                .set_title(i18n::t("dialog.select_clone_dir"))
                .pick_folder()
            {
                if let Some(app) = ah_select.upgrade() {
                    let target_name = app.get_clone_target_name().to_string();
                    let final_path = path.join(target_name).to_string_lossy().to_string();
                    app.set_clone_target_path(final_path.into());
                    app.set_clone_base_path(path.to_string_lossy().to_string().into());
                }
            }
        });
    }

    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_confirm_clone(move |source_name, target_name, target_path| {
            info!("Operation: Confirm clone - Source: {}, Target: {}, Path: {}", source_name, target_name, target_path);
            let ah_weak = ah_outer.clone();
            let as_ptr_outer = as_outer.clone();
            
            let _ = slint::spawn_local(async move {
                let manager = {
                    let state = as_ptr_outer.lock().await;
                    state.wsl_dashboard.clone()
                };

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
                    let app: AppWindow = app;
                    if app.get_is_cloning() || app.get_is_exporting() || app.get_is_moving() {
                        return;
                    }

                    // 1. Validation: Name length <= 24
                    if target_name.len() > 24 {
                        error!("Clone failed: name too long");
                        app.set_clone_error(i18n::t("dialog.name_too_long").into());
                        return;
                    }

                    // 2. Validation: ASCII Alphanumeric and -_.
                    let is_valid_name = target_name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
                    if !is_valid_name {
                        app.set_clone_error(i18n::t("dialog.name_invalid").into());
                        return;
                    }

                    // 3. Validation: Instance exists
                    let distros = app.get_distros();
                    for i in 0..distros.row_count() {
                        if let Some(d) = distros.row_data(i) {
                            if d.name == target_name {
                                app.set_clone_error(i18n::t("dialog.name_exists").into());
                                return;
                            }
                        }
                    }

                    // 4. Validation: Directory emptiness
                    let p = std::path::Path::new(target_path.as_str());
                    if p.exists() {
                        if p.is_dir() {
                            if let Ok(entries) = std::fs::read_dir(p) {
                                if entries.count() > 0 {
                                    app.set_clone_error(i18n::t("dialog.dir_not_empty").into());
                                    return;
                                }
                            }
                        } else {
                            app.set_clone_error(i18n::t("dialog.path_is_not_dir").into());
                            return;
                        }
                    } else {
                        if let Err(e) = std::fs::create_dir_all(p) {
                            app.set_clone_error(i18n::tr("dialog.mkdir_failed", &[e.to_string()]).into());
                            return;
                        }
                    }

                    app.set_clone_error("".into());
                    app.set_show_clone_dialog(false);
                    
                    app.set_is_cloning(true);
                    
                    let ah_clone = app.as_weak();
                    let as_ptr = as_ptr_outer.clone();
                    let source_name_inner = source_name.to_string();
                    let target_name_inner = target_name.to_string();
                    let target_path_inner = target_path.to_string();

                    let _ = tokio::spawn(async move {
                        let manager = {
                            let state = as_ptr.lock().await;
                            state.wsl_dashboard.clone()
                        };

                        if let Some(op) = manager.get_active_op(&source_name_inner).await {
                            let msg = i18n::tr("toast.distro_busy", &[source_name_inner.clone(), op.to_string()]);
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(app) = ah_clone.upgrade() {
                                    app.set_current_message(msg.into());
                                    app.set_show_message_dialog(true);
                                    app.set_is_cloning(false);
                                }
                            });
                            return;
                        }

                        super::clone_logic::perform_clone(ah_clone, as_ptr, source_name_inner, target_name_inner, target_path_inner).await;
                    });
                }
            });
        });
    }

    {
        let ah_name = app_handle.clone();
        app.on_clone_name_changed(move |new_name| {
            if let Some(app) = ah_name.upgrade() {
                let base_path = app.get_clone_base_path().to_string();
                if base_path.is_empty() { return; }
                
                let new_path = std::path::Path::new(&base_path)
                    .join(new_name.to_string())
                    .to_string_lossy()
                    .to_string();
                app.set_clone_target_path(new_path.into());
            }
        });
    }

    app.on_close_message_dialog(move || {
        // Placeholder
    });
}
