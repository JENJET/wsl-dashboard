use std::sync::Arc;
use tokio::sync::Mutex;
use crate::{AppState, AppWindow};
use crate::network;
use super::utils::show_toast;

pub fn setup(app: &AppWindow, app_handle: slint::Weak<AppWindow>, _app_state: Arc<Mutex<AppState>>) {
    let ah_check = app_handle.clone();
    app.on_check_network_task_status(move || {
        let ah = ah_check.clone();
        let exists = network::scheduler::check_task_exists();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = ah.upgrade() {
                app.set_network_is_helper_installed(exists);
            }
        });
    });

    let ah_run = app_handle.clone();
    app.on_initialize_task_clicked(move || {
        let ah = ah_run.clone();
        tokio::spawn(async move {
            match network::scheduler::register_task_with_elevation() {
                Ok(_) => {
                    let ah_success = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_success.upgrade() {
                            app.set_network_is_helper_installed(true);
                        }
                    });
                    show_toast(ah, crate::i18n::t("network.task_scheduled"));
                },
                Err(e) => {
                    let msg = if e.contains("InvalidOperation") || e.contains("denied") {
                        crate::i18n::t("network.error_uac")
                    } else {
                        crate::i18n::tr("network.task_failed", &[e.to_string()])
                    };
                    show_toast(ah, msg);
                }
            }
        });
    });

    let ah_cancel = app_handle.clone();
    app.on_uninitialize_task_clicked(move || {
        let ah = ah_cancel.clone();
        tokio::spawn(async move {
            match network::scheduler::unregister_task_with_elevation() {
                Ok(_) => {
                    let ah_success = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_success.upgrade() {
                            app.set_network_is_helper_installed(false);
                        }
                    });
                    show_toast(ah, crate::i18n::t("network.task_unscheduled"));
                },
                Err(e) => {
                    let msg = if e.contains("InvalidOperation") || e.contains("denied") {
                        crate::i18n::t("network.error_uac")
                    } else {
                        crate::i18n::tr("network.task_failed", &[e.to_string()])
                    };
                    show_toast(ah, msg);
                }
            }
        });
    });
}
