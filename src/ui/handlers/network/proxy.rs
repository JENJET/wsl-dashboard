use crate::network;
use crate::{AppState, AppWindow};
use std::sync::Arc;
use tokio::sync::Mutex;

pub fn setup(app: &AppWindow, app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    app.set_network_proxy_default_host(network::models::default_host().into());
    app.set_network_proxy_default_port(network::models::default_port().into());
    app.set_network_proxy_default_no_proxy(network::models::default_no_proxy().into());

    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_load_proxy_settings(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        tokio::spawn(async move {
            let config = {
                let state = as_ptr.lock().await;
                state.config_manager.get_network_config().proxy.clone()
            };
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah.upgrade() {
                    let no_proxy = if config.no_proxy.is_empty() {
                        network::models::default_no_proxy()
                    } else {
                        config.no_proxy.clone()
                    };

                    app.set_network_proxy_is_enabled(config.is_enabled);
                    app.set_network_proxy_host(config.host.clone().into());
                    app.set_network_proxy_port(config.port.clone().into());
                    app.set_network_proxy_auth_enabled(config.auth_enabled);
                    app.set_network_proxy_username(config.username.into());
                    app.set_network_proxy_password(config.password.into());
                    app.set_network_proxy_no_proxy(no_proxy.into());
                    app.set_network_proxy_default_host(network::models::default_host().into());
                    app.set_network_proxy_default_port(network::models::default_port().into());
                    app.set_network_proxy_default_no_proxy(
                        network::models::default_no_proxy().into(),
                    );
                }
            });
        });
    });

    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_save_proxy_settings(move |enabled, host, port, no_proxy, auth, user, pass| {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let host = host.to_string();
        let port_str = port.to_string();
        let no_proxy = no_proxy.to_string();
        let user = user.to_string();
        let pass = pass.to_string();

        // Validation: Port range 1-65534 (allow empty)
        let port_valid = if port_str.is_empty() {
            true
        } else if let Ok(p) = port_str.parse::<u32>() {
            p > 0 && p < 65535
        } else {
            false
        };

        if !port_valid {
            let ah_err = ah.clone();
            let ah_timer = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_err.upgrade() {
                    let err_msg = crate::i18n::t("network.proxy_error_port");
                    app.set_task_status_text(err_msg.into());
                    app.set_task_status_visible(true);
                }
                slint::Timer::single_shot(std::time::Duration::from_secs(3), move || {
                    if let Some(app) = ah_timer.upgrade() {
                        app.set_task_status_visible(false);
                    }
                });
            });
            return;
        }

        let port = port_str;

        // Validation: No localhost or 127.0.0.1
        if host == "localhost" || host == "127.0.0.1" {
            let ah_err = ah.clone();
            let ah_timer = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_err.upgrade() {
                    let err_msg = crate::i18n::t("network.proxy_error_localhost");
                    app.set_task_status_text(err_msg.into());
                    app.set_task_status_visible(true);
                }
                slint::Timer::single_shot(std::time::Duration::from_secs(3), move || {
                    if let Some(app) = ah_timer.upgrade() {
                        app.set_task_status_visible(false);
                    }
                });
            });
            return;
        }

        tokio::spawn(async move {
            tracing::info!(
                "Updating HTTP proxy settings: enabled={}, host={}, port={}",
                enabled,
                host,
                port
            );
            let state = as_ptr.lock().await;
            let mut net_config = state.config_manager.get_network_config().clone();
            net_config.proxy.is_enabled = enabled;
            net_config.proxy.host = host.clone();
            net_config.proxy.port = port.clone();
            net_config.proxy.auth_enabled = auth;
            net_config.proxy.username = user.clone();
            net_config.proxy.password = pass.clone();
            net_config.proxy.no_proxy = no_proxy.clone();

            let _ = state.config_manager.update_network_config(net_config);

            let ah_status = ah.clone();
            let ah_timer = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_status.upgrade() {
                    let success_msg = crate::i18n::t("network.proxy_save_success");
                    app.set_task_status_text(success_msg.into());
                    app.set_task_status_visible(true);
                }
                slint::Timer::single_shot(std::time::Duration::from_secs(3), move || {
                    if let Some(app) = ah_timer.upgrade() {
                        app.set_task_status_visible(false);
                    }
                });
            });
        });
    });

    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_check_proxy_connection(move |url| {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let url = url.to_string();

        let app = match ah.upgrade() {
            Some(a) => a,
            None => return,
        };
        let host = app.get_network_proxy_host().to_string();
        let port = app.get_network_proxy_port().to_string();
        let auth = app.get_network_proxy_auth_enabled();
        let user = app.get_network_proxy_username().to_string();
        let pass = app.get_network_proxy_password().to_string();
        let no_proxy = app.get_network_proxy_no_proxy().to_string();

        tracing::info!(
            "Testing proxy connection to {} using proxy {}:{}",
            url,
            host,
            port
        );
        tokio::spawn(async move {
            let proxy_config = network::models::HttpProxyConfig {
                is_enabled: true,
                host: host.clone(),
                port: port.clone(),
                auth_enabled: auth,
                username: user.clone(),
                password: pass.clone(),
                no_proxy: no_proxy.clone(),
            };

            {
                let state = as_ptr.lock().await;
                let mut net_config = state.config_manager.get_network_config().clone();
                net_config.proxy = proxy_config.clone();
                let _ = state.config_manager.update_network_config(net_config);
            }

            let mut proxy_str = format!("http://{}:{}", proxy_config.host, proxy_config.port);

            if proxy_config.auth_enabled {
                let user_enc = urlencoding::encode(&proxy_config.username);
                let pass_enc = urlencoding::encode(&proxy_config.password);
                proxy_str = format!(
                    "http://{}:{}@{}:{}",
                    user_enc, pass_enc, proxy_config.host, proxy_config.port
                );
            }

            let proxy_obj = match ureq::Proxy::new(&proxy_str) {
                Ok(p) => p,
                Err(e) => {
                    let ah_err = ah.clone();
                    let ah_timer = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_err.upgrade() {
                            let err_msg =
                                crate::i18n::tr("network.proxy_error_invalid", &[e.to_string()]);
                            app.set_task_status_text(err_msg.into());
                            app.set_task_status_visible(true);
                        }
                        slint::Timer::single_shot(std::time::Duration::from_secs(4), move || {
                            if let Some(app) = ah_timer.upgrade() {
                                app.set_task_status_visible(false);
                            }
                        });
                    });
                    return;
                }
            };

            let agent = ureq::AgentBuilder::new()
                .proxy(proxy_obj)
                .timeout(std::time::Duration::from_secs(8))
                .build();

            let res = agent.get(&url).call();

            let result_msg = match res {
                Ok(response) => crate::i18n::tr(
                    "network.proxy_test_success",
                    &[response.status().to_string()],
                ),
                Err(e) => crate::i18n::tr("network.proxy_test_failed", &[e.to_string()]),
            };

            tracing::info!("Proxy test result for {}: {}", url, result_msg);

            let ah_status = ah.clone();
            let ah_timer = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_status.upgrade() {
                    app.set_task_status_text(result_msg.into());
                    app.set_task_status_visible(true);
                }
                slint::Timer::single_shot(std::time::Duration::from_secs(4), move || {
                    if let Some(app) = ah_timer.upgrade() {
                        app.set_task_status_visible(false);
                    }
                });
            });
        });
    });
}
