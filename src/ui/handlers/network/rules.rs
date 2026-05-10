use super::utils::{refresh_network_view_data, show_toast};
use crate::network;
use crate::{AppState, AppWindow};
use std::sync::Arc;
use tokio::sync::Mutex;

pub fn setup(app: &AppWindow, app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    let ah_open = app_handle.clone();
    app.on_open_add_rule_dialog(move || {
        if let Some(app) = ah_open.upgrade() {
            app.set_network_show_add_dialog(true);
            app.set_network_add_error("".into());
            // Default values: 8080 for ports, firewall OFF
            app.set_network_add_listen_port("8080".into());
            app.set_network_add_target_port("8080".into());
            app.set_network_add_enable_fw(false);
            app.set_network_add_distro_idx(0);
            app.set_network_add_ip_idx(0);
        }
    });

    let ah_add = app_handle.clone();
    let as_add = app_state.clone();
    app.on_add_network_rule(move |distro, ip, lport, tport, fw| {
        let ah = ah_add.clone();
        let as_ptr = as_add.clone();

        let lport_str = lport.to_string();
        let tport_str = tport.to_string();

        // Validation: Port range 1-65534
        let lport_valid = if let Ok(p) = lport_str.parse::<u32>() {
            p > 0 && p < 65535
        } else {
            false
        };
        let tport_valid = if let Ok(p) = tport_str.parse::<u32>() {
            p > 0 && p < 65535
        } else {
            false
        };

        if !lport_valid || !tport_valid {
            if let Some(app) = ah.upgrade() {
                let err_msg = crate::i18n::t("network.proxy_error_port");
                app.set_network_add_error(err_msg.into());
            }
            return;
        }

        tokio::spawn(async move {
            let state = as_ptr.lock().await;

            // 1. Check for duplicates
            let current_config = state.config_manager.get_network_config();
            let listen_port_num = lport.parse::<u16>().unwrap_or(0);

            let is_duplicate = current_config.port_proxies.iter().any(|r| {
                r.listen_address == ip.as_str() && r.listen_port == listen_port_num
            });

            if is_duplicate {
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah.upgrade() {
                        let err_msg = crate::i18n::t("network.error_duplicate");
                        app.set_network_add_error(err_msg.into());
                    }
                });
                return;
            }

            let mut net_config = current_config;
            let rule = network::models::PortProxyRule {
                id: chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0).to_string(),
                distro_name: distro.to_string(),
                listen_address: ip.to_string(),
                listen_port: listen_port_num,
                target_port: tport.parse().unwrap_or(0),
                enable_firewall: fw,
            };
            if rule.listen_port > 0 && rule.target_port > 0 {
                let log_distro = rule.distro_name.clone();
                let log_addr = rule.listen_address.clone();
                let log_lport = rule.listen_port;
                let log_tport = rule.target_port;
                let log_fw = rule.enable_firewall;

                net_config.port_proxies.push(rule);
                if let Err(e) = state.config_manager.update_network_config(net_config) {
                    tracing::error!("Failed to save network configuration: {}", e);
                    let err_msg = crate::i18n::tr("network.save_config_failed", &[e.to_string()]);
                    let ah_err = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_err.upgrade() {
                            app.set_network_add_error(err_msg.into());
                        }
                    });
                    return;
                } else {
                    tracing::info!("Added network rule to config: {} {}:{} -> {}", log_distro, log_addr, log_lport, log_tport);

                    // INSTANT APPLY: Trigger single elevation for BOTH proxy and firewall
                    // Scenario A: If distro is NOT started, we don't start it, don't get IP, don't call netsh portproxy.
                    // Scenario B: If distro IS started, execute existing logic.
                    let is_running = state.wsl_dashboard.is_distro_running(&log_distro).await;
                    if is_running {
                        match network::tracker::get_distro_ip(&log_distro, None) {
                            Ok(target_ip) => {
                                let _ = network::port_proxy::add_port_proxy_and_firewall_elevated(
                                    &log_addr, log_lport, &target_ip, log_tport,
                                    log_fw, &log_distro
                                );
                                tracing::info!("Instant rule apply initiated (Distro Running) for {}:{}", log_addr, log_lport);
                            }
                            Err(e) => {
                                tracing::warn!("Could not obtain IP for instant apply: {}. Rule saved but inactive.", e);
                            }
                        }
                    } else {
                        tracing::info!("Distro {} is not running (from state). Skipping instant portproxy apply. Rule saved.", log_distro);
                        if log_fw {
                            let _ = network::port_proxy::add_firewall_rule_elevated(&log_addr, log_lport, &log_distro);
                            tracing::info!("Firewall rule applied for inactive distro {}.", log_distro);
                        }
                    }


                }
            }
            drop(state);
            let ah_finish = ah.clone();
            refresh_network_view_data(ah, as_ptr).await;

            // Success: Close the dialog
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_finish.upgrade() {
                    app.set_network_show_add_dialog(false);
                }
            });
        });
    });

    let ah_process = app_handle.clone();
    let as_process = app_state.clone();
    app.on_process_network_rule(move |id| {
        let ah = ah_process.clone();
        let as_ptr = as_process.clone();
        tokio::spawn(async move {
            let state = as_ptr.lock().await;
            let net_config = state.config_manager.get_network_config();
            if let Some(rule) = net_config.port_proxies.iter().find(|r| r.id == id.as_str()) {
                let distro_name = rule.distro_name.clone();
                let listen_addr = rule.listen_address.clone();
                let listen_port = rule.listen_port;
                let target_port = rule.target_port;
                let enable_fw = rule.enable_firewall;
                let dashboard = state.wsl_dashboard.clone();
                drop(state);

                // 1. Check if distro is running using app state (Home list logic)
                if !dashboard.is_distro_running(&distro_name).await {
                    let err_msg = crate::i18n::tr(
                        "network.rules_error_not_running",
                        &[distro_name.to_string()],
                    );
                    show_toast(ah, err_msg);
                    return;
                }

                // 2. Get current IP if running
                tracing::info!(
                    "Manually applying rule for {}:{} -> target distro: {}",
                    listen_addr,
                    listen_port,
                    distro_name
                );
                match network::tracker::get_distro_ip(&distro_name, None) {
                    Ok(ip) => {
                        match network::port_proxy::add_port_proxy_elevated(
                            &listen_addr,
                            listen_port,
                            &ip,
                            target_port,
                            enable_fw,
                            &distro_name,
                        ) {
                            Ok(_) => {
                                tracing::info!(
                                    "Applied port proxy: {}:{} -> {}:{} (Firewall: {})",
                                    listen_addr,
                                    listen_port,
                                    ip,
                                    target_port,
                                    enable_fw
                                );
                                let success_msg = crate::i18n::tr(
                                    "network.rules_apply_success",
                                    &[listen_port.to_string(), ip.to_string()],
                                );
                                show_toast(ah.clone(), success_msg);
                                refresh_network_view_data(ah, as_ptr).await;
                            }
                            Err(e) => {
                                let msg = if e.contains("InvalidOperation") || e.contains("denied")
                                {
                                    crate::i18n::t("network.error_uac")
                                } else {
                                    crate::i18n::tr("network.error_uac_detail", &[e.to_string()])
                                };
                                show_toast(ah, msg);
                            }
                        }
                    }
                    Err(_e) => {
                        let err_msg = crate::i18n::tr(
                            "network.rules_error_no_ip",
                            &[distro_name.to_string()],
                        );
                        show_toast(ah, err_msg);
                    }
                }
            }
        });
    });

    let ah_del = app_handle.clone();
    let as_del = app_state.clone();
    app.on_delete_network_rule(move |id| {
        let ah = ah_del.clone();
        let as_ptr = as_del.clone();
        tokio::spawn(async move {
            let state = as_ptr.lock().await;
            let mut net_config = state.config_manager.get_network_config();

            // Physical deletion from Windows with UAC
            if let Some(rule) = net_config.port_proxies.iter().find(|r| r.id == id.as_str()) {
                let _ = network::port_proxy::delete_port_proxy_and_firewall_elevated(
                    &rule.listen_address,
                    rule.listen_port,
                    &rule.distro_name,
                );
            }

            net_config.port_proxies.retain(|r| r.id != id.as_str());
            if let Err(e) = state.config_manager.update_network_config(net_config) {
                tracing::error!("Failed to save network configuration: {}", e);
            } else {
                tracing::info!("Deleted rule {} from config and system", id);
            }
            drop(state);
            refresh_network_view_data(ah, as_ptr).await;
        });
    });

    // Cancel a single active rule (remove from system but keep in config)
    let ah_cancel = app_handle.clone();
    let as_cancel = app_state.clone();
    app.on_cancel_network_rule(move |id| {
        let ah = ah_cancel.clone();
        let as_ptr = as_cancel.clone();
        tokio::spawn(async move {
            let state = as_ptr.lock().await;
            let net_config = state.config_manager.get_network_config();
            if let Some(rule) = net_config.port_proxies.iter().find(|r| r.id == id.as_str()) {
                let listen_addr = rule.listen_address.clone();
                let listen_port = rule.listen_port;
                let distro_name = rule.distro_name.clone();
                drop(state);

                tracing::info!(
                    "Canceling active rule for {}:{} (distro: {})",
                    listen_addr,
                    listen_port,
                    distro_name
                );
                match network::port_proxy::delete_port_proxy_elevated(
                    &listen_addr,
                    listen_port,
                    &distro_name,
                ) {
                    Ok(_) => {
                        refresh_network_view_data(ah, as_ptr).await;
                    }
                    Err(e) => {
                        let msg = if e.contains("InvalidOperation") || e.contains("denied") {
                            crate::i18n::t("network.error_uac")
                        } else {
                            crate::i18n::tr("network.rules_cancel_failed", &[e.to_string()])
                        };
                        show_toast(ah, msg);
                    }
                }
            }
        });
    });

    let ah_apply_all = app_handle.clone();
    let as_apply_all = app_state.clone();
    app.on_apply_all_network_rules(move || {
        let ah = ah_apply_all.clone();
        let as_ptr = as_apply_all.clone();
        tokio::spawn(async move {
            let active_ports = network::port_proxy::get_active_listen_ports().unwrap_or_default();
            let (net_config, ah_clone, dashboard) = {
                let state = as_ptr.lock().await;
                (
                    state.config_manager.get_network_config().clone(),
                    ah.clone(),
                    state.wsl_dashboard.clone(),
                )
            };

            let mut rules_to_apply = Vec::new();
            for rule in net_config.port_proxies {
                if !active_ports.contains(&(rule.listen_address.clone(), rule.listen_port)) {
                    // Check if distro is running before trying to get IP (Avoid starting the distro)
                    if dashboard.is_distro_running(&rule.distro_name).await {
                        if let Ok(ip) = network::tracker::get_distro_ip(&rule.distro_name, None) {
                            rules_to_apply.push((rule.clone(), ip));
                        }
                    } else {
                        tracing::info!(
                            "Skipping apply all for {}:{} because distro {} is not running.",
                            rule.listen_address,
                            rule.listen_port,
                            rule.distro_name
                        );
                    }
                }
            }

            if rules_to_apply.is_empty() {
                tracing::info!("Apply all: No inactive rules to apply.");
                return;
            }

            tracing::info!(
                "Applying all inactive rules (count: {})",
                rules_to_apply.len()
            );
            match network::port_proxy::apply_port_proxies_elevated(rules_to_apply) {
                Ok(_) => {
                    refresh_network_view_data(ah_clone, as_ptr).await;
                }
                Err(e) => {
                    let msg = if e.contains("InvalidOperation") || e.contains("denied") {
                        crate::i18n::t("network.error_uac")
                    } else {
                        crate::i18n::tr("network.rules_apply_all_failed", &[e.to_string()])
                    };
                    show_toast(ah_clone, msg);
                }
            }
        });
    });

    let ah_cancel_all = app_handle.clone();
    let as_cancel_all = app_state.clone();
    app.on_cancel_all_network_rules(move || {
        let ah = ah_cancel_all.clone();
        let as_ptr = as_cancel_all.clone();
        tokio::spawn(async move {
            let active_ports = network::port_proxy::get_active_listen_ports().unwrap_or_default();
            let (net_config, ah_clone) = {
                let state = as_ptr.lock().await;
                (
                    state.config_manager.get_network_config().clone(),
                    ah.clone(),
                )
            };

            let mut rules_to_cancel = Vec::new();
            for rule in net_config.port_proxies {
                if active_ports.contains(&(rule.listen_address.clone(), rule.listen_port)) {
                    rules_to_cancel.push(rule.clone());
                }
            }

            if rules_to_cancel.is_empty() {
                tracing::info!("Cancel all: No active rules to cancel.");
                return;
            }

            tracing::info!(
                "Canceling all active rules (count: {})",
                rules_to_cancel.len()
            );
            let rules_count = rules_to_cancel.len();
            match network::port_proxy::delete_port_proxies_elevated(rules_to_cancel) {
                Ok(_) => {
                    tracing::info!("Successfully canceled all {} rules", rules_count);
                    refresh_network_view_data(ah_clone, as_ptr).await;
                }
                Err(e) => {
                    let msg = if e.contains("InvalidOperation") || e.contains("denied") {
                        crate::i18n::t("network.error_uac")
                    } else {
                        crate::i18n::tr("network.rules_cancel_all_failed", &[e.to_string()])
                    };
                    show_toast(ah_clone, msg);
                }
            }
        });
    });
}
