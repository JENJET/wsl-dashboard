pub mod about;
pub mod common;
pub mod distro;
pub mod instance;
pub mod network;
pub mod settings;
pub mod update;
pub mod usb;
pub mod window;
pub mod wsl_manage;

use crate::i18n;
use crate::wsl::terminal;
use crate::{AppState, AppWindow};
use slint::Weak;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Resolve terminal preset and proxy exports for a given distro,
/// then open the terminal via the executor.
pub async fn resolve_and_open_terminal(
    app_state: &Arc<Mutex<AppState>>,
    distro_name: &str,
    ah: &Weak<AppWindow>,
) {
    let lock_timeout = std::time::Duration::from_millis(500);
    if let Ok(guard) = tokio::time::timeout(lock_timeout, app_state.lock()).await {
        let executor = guard.wsl_dashboard.executor().clone();
        let instance_config = guard.config_manager.get_instance_config(distro_name);
        let working_dir = instance_config.terminal_dir.clone();
        let terminal_proxy_enabled = instance_config.terminal_proxy;
        let proxy_config = guard.config_manager.get_network_config().proxy.clone();
        let global_settings = guard.config_manager.get_settings().clone();
        drop(guard);

        let terminal_name = if instance_config.terminal_emulator.is_empty() {
            global_settings.terminal_emulator.clone()
        } else {
            instance_config.terminal_emulator.clone()
        };

        let builtin = terminal::get_builtin_presets_map();
        let all_presets = terminal::resolve_presets(
            builtin,
            &global_settings.terminal_presets,
            &global_settings.terminal_user_presets,
        );
        let terminal_preset = all_presets
            .get(&terminal_name)
            .unwrap_or_else(|| {
                all_presets
                    .get(terminal::BuiltinTerminal::Cmd.as_str())
                    .expect("builtin cmd preset must exist")
            })
            .clone();

        let proxy_exports = build_proxy_exports(terminal_proxy_enabled, &proxy_config);

        let result = executor
            .open_distro_terminal(distro_name, &working_dir, proxy_exports, &terminal_preset)
            .await;

        if let Some(err) = result.error {
            let error_msg = i18n::tr("dialog.terminal.terminal_open_failed", &[err]);
            let ah2 = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah2.upgrade() {
                    app.set_current_message(error_msg.into());
                    app.set_show_message_dialog(true);
                }
            });
        }
    }
}

fn build_proxy_exports(
    terminal_proxy_enabled: bool,
    proxy_config: &crate::network::models::HttpProxyConfig,
) -> Option<Vec<(String, String)>> {
    if terminal_proxy_enabled
        && proxy_config.is_enabled
        && !proxy_config.host.is_empty()
        && !proxy_config.port.is_empty()
    {
        let auth = if proxy_config.auth_enabled
            && !proxy_config.username.is_empty()
            && !proxy_config.password.is_empty()
        {
            format!("{}:{}@", proxy_config.username, proxy_config.password)
        } else {
            String::new()
        };
        let proxy_url = format!("http://{}{}:{}", auth, proxy_config.host, proxy_config.port);
        let mut exports = Vec::new();
        exports.push(("HTTP_PROXY".to_string(), proxy_url.clone()));
        exports.push(("HTTPS_PROXY".to_string(), proxy_url));
        if !proxy_config.no_proxy.is_empty() {
            exports.push(("NO_PROXY".to_string(), proxy_config.no_proxy.clone()));
        }
        Some(exports)
    } else {
        None
    }
}

pub async fn setup(
    app: &AppWindow,
    app_handle: slint::Weak<AppWindow>,
    app_state: Arc<Mutex<AppState>>,
) {
    common::setup(app, app_handle.clone(), app_state.clone());
    window::setup(app, app_handle.clone(), app_state.clone());
    distro::setup(app, app_handle.clone(), app_state.clone());
    settings::setup(app, app_handle.clone(), app_state.clone());
    update::setup(app, app_handle.clone(), app_state.clone());
    instance::setup(app, app_handle.clone(), app_state.clone());
    usb::setup(app, app_handle.clone(), app_state.clone());
    network::setup(app, app_handle.clone(), app_state.clone());
    about::setup(app, app_handle.clone(), app_state.clone());
    wsl_manage::setup(app, app_handle.clone(), app_state.clone());
}
