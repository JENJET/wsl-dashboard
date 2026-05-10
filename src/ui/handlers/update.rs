use crate::{AppInfo, AppState, AppWindow};
use slint::ComponentHandle;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

pub fn setup(
    app: &AppWindow,
    app_handle: slint::Weak<AppWindow>,
    _app_state: Arc<Mutex<AppState>>,
) {
    let ah = app_handle.clone();
    let as_check = _app_state.clone();
    let as_download = _app_state.clone();

    app.on_check_update(move || {
        info!("Manual check update triggered from UI");
        let ah = ah.clone();

        // Directly use the compile-time version number constant, or obtain the version number on the UI thread.
        // This eliminates the need to call ah.upgrade() on a background thread.
        let current_v = env!("CARGO_PKG_VERSION").to_string();

        let as_ptr = as_check.clone();
        tokio::spawn(async move {
            info!("Starting manual check for version: {}", current_v);

            // Set checking_update to true on the UI thread
            let _ = slint::invoke_from_event_loop({
                let ah = ah.clone();
                move || {
                    if let Some(app) = ah.upgrade() {
                        app.global::<AppInfo>().set_checking_update(true);
                    }
                }
            });

            let timezone = {
                let state = as_ptr.lock().await;
                state.config_manager.get_config().system.timezone.clone()
            };

            match crate::app::updater::check_update(&current_v, &timezone).await {
                Ok(result) => {
                    let has_update = result.has_update;
                    let latest_version = result.latest_version.clone();

                    let _ = slint::invoke_from_event_loop({
                        let ah = ah.clone();
                        move || {
                            if let Some(app) = ah.upgrade() {
                                info!("Update check result: has_update={}", has_update);
                                app.global::<AppInfo>().set_checking_update(false);
                                app.global::<AppInfo>().set_has_update(has_update);
                                app.global::<AppInfo>()
                                    .set_latest_version(latest_version.into());

                                if has_update {
                                    app.set_show_update_dialog(true);
                                } else {
                                    app.set_current_message(
                                        crate::i18n::t("dialog.update_latest").into(),
                                    );
                                    app.set_show_message_dialog(true);
                                }
                            }
                        }
                    });
                }
                Err(e) => {
                    warn!("Manual check update failed: {}", e);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah.upgrade() {
                            app.global::<AppInfo>().set_checking_update(false);

                            let message = if e == "RequestTimeOut" {
                                crate::i18n::t("dialog.update_timeout")
                            } else {
                                crate::i18n::tr("dialog.update_failed", &[e])
                            };

                            app.set_current_message(message.into());
                            app.set_show_message_dialog(true);
                        }
                    });
                }
            }
        });
    });

    app.on_download_update(move || {
        let as_ptr = as_download.clone();
        slint::spawn_local(async move {
            let timezone = {
                let state = as_ptr.lock().await;
                state.config_manager.get_config().system.timezone.clone()
            };

            let base_github_url = if timezone == crate::app::ZH_TIMEZONE {
                crate::app::GITEE_URL
            } else {
                crate::app::GITHUB_URL
            };
            let _ = open::that(format!(
                "{}{}",
                base_github_url,
                crate::app::GITHUB_RELEASES
            ));
        })
        .unwrap();
    });

    let ah = app_handle.clone();
    app.on_close_expire_dialog(move || {
        if let Some(app) = ah.upgrade() {
            app.set_show_expire_dialog(false);
        }
    });
}
