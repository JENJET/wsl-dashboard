use crate::{AppState, AppWindow, i18n};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

pub fn setup(app: &AppWindow, app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    // Start
    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_start_distro(move |name| {
            info!("Operation: Start distribution - {}", name);
            let ah = ah_outer.clone();
            let as_ptr = as_outer.clone();
            tokio::spawn(async move {
                let manager = {
                    let app_state = as_ptr.lock().await;
                    app_state.wsl_dashboard.clone()
                };

                // Sentinel Check: Distro busy?
                if let Some(op) = manager.get_active_op(&name) {
                    let msg = i18n::tr("toast.distro_busy", &[name.to_string(), op.to_string()]);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah.upgrade() {
                            app.set_current_message(msg.into());
                            app.set_show_message_dialog(true);
                        }
                    });
                    return;
                }

                let ah_status = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_status.upgrade() {
                        app.set_task_status_text(i18n::t("operation.starting").into());
                        app.set_task_status_visible(true);
                    }
                });

                let result = manager.start_distro(&name).await;
                if result.success {
                    let n = name.to_string();
                    let d = manager.clone();
                    tokio::spawn(async move {
                        super::maybe_setup_fstrim(d.executor(), &n).await;
                    });
                }
                let ah_res = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_res.upgrade() {
                        app.set_task_status_visible(false);
                    }
                });
            });
        });
    }

    // Stop
    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_stop_distro(move |name| {
            info!("Operation: Stop distribution - {}", name);
            let ah = ah_outer.clone();
            let as_ptr = as_outer.clone();
            tokio::spawn(async move {
                let manager = {
                    let app_state = as_ptr.lock().await;
                    app_state.wsl_dashboard.clone()
                };

                // Sentinel Check: Distro busy?
                if let Some(op) = manager.get_active_op(&name) {
                    let msg = i18n::tr("toast.distro_busy", &[name.to_string(), op.to_string()]);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah.upgrade() {
                            app.set_current_message(msg.into());
                            app.set_show_message_dialog(true);
                        }
                    });
                    return;
                }

                let ah_status = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_status.upgrade() {
                        app.set_task_status_text(i18n::t("operation.stopping").into());
                        app.set_task_status_visible(true);
                    }
                });

                manager.stop_distro(&name).await;
                let ah_res = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_res.upgrade() {
                        app.set_task_status_visible(false);
                    }
                });
            });
        });
    }

    // Restart
    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_restart_distro(move |name| {
            info!("Operation: Restart distribution - {}", name);
            let ah = ah_outer.clone();
            let as_ptr = as_outer.clone();
            tokio::spawn(async move {
                let manager = {
                    let app_state = as_ptr.lock().await;
                    app_state.wsl_dashboard.clone()
                };

                // Sentinel Check: Distro busy?
                if let Some(op) = manager.get_active_op(&name) {
                    let msg = i18n::tr("toast.distro_busy", &[name.to_string(), op.to_string()]);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah.upgrade() {
                            app.set_current_message(msg.into());
                            app.set_show_message_dialog(true);
                        }
                    });
                    return;
                }

                let ah_status = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_status.upgrade() {
                        app.set_task_status_text(i18n::t("operation.restarting").into());
                        app.set_task_status_visible(true);
                    }
                });

                manager.restart_distro(&name).await;
                let ah_res = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_res.upgrade() {
                        app.set_task_status_visible(false);
                    }
                });
            });
        });
    }

    // Delete
    {
        let ah_outer = app_handle.clone();
        let as_outer = app_state.clone();
        app.on_delete_distro(move |name| {
            info!("Operation: Delete distribution - {}", name);
            let ah = ah_outer.clone();
            let as_ptr = as_outer.clone();

            if let Some(app) = ah.upgrade() {
                if app.get_is_exporting()
                    || app.get_is_cloning()
                    || app.get_is_moving()
                    || app.get_is_installing()
                {
                    app.set_current_message(i18n::t("dialog.operation_in_progress").into());
                    app.set_show_message_dialog(true);
                    return;
                }
            }

            tokio::spawn(async move {
                let (dashboard, config_manager) = {
                    let app_state = as_ptr.lock().await;
                    (
                        app_state.wsl_dashboard.clone(),
                        app_state.config_manager.clone(),
                    )
                };

                // Sentinel Check: Distro busy?
                if let Some(op) = dashboard.get_active_op(&name) {
                    let msg = i18n::tr("toast.distro_busy", &[name.to_string(), op.to_string()]);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah.upgrade() {
                            app.set_current_message(msg.into());
                            app.set_show_message_dialog(true);
                        }
                    });
                    return;
                }

                // Sentinel Check: System heavy op?
                if dashboard.heavy_op_lock().try_lock().is_err() {
                    let msg = i18n::t("toast.system_busy");
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah.upgrade() {
                            app.set_current_message(msg.into());
                            app.set_show_message_dialog(true);
                        }
                    });
                    return;
                }

                let ah_init = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_init.upgrade() {
                        app.set_task_status_text(i18n::t("operation.deleting").into());
                        app.set_task_status_visible(true);
                    }
                });

                dashboard.delete_distro(&config_manager, &name).await;

                let ah_final = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_final.upgrade() {
                        app.set_task_status_visible(false);
                    }
                });
            });
        });
    }

    // Delete confirmation log
    app.on_delete_clicked(move |name| {
        info!("Operation: Open delete confirmation - {}", name);
    });
}
