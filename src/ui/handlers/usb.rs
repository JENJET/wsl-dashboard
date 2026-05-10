use crate::utils::system::copy_to_clipboard;
use crate::{AppI18n, AppState, AppWindow, UsbDevice};
use serde_json::json;
use slint::ComponentHandle;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

/// Convert backend USB device model to Slint model
fn to_slint_device(
    dev: &crate::usb::UsbDeviceModel,
    auto_attach_list: &[crate::config::models::UsbAutoAttachDevice],
) -> UsbDevice {
    let full_instance_id = dev.instance_id.clone().unwrap_or_default();
    let sn_part = full_instance_id
        .split('\\')
        .last()
        .unwrap_or("")
        .to_string();
    let is_real_sn = !sn_part.contains('&') && !sn_part.is_empty();
    let persisted_guid = dev.persisted_guid.clone().unwrap_or_default();

    // Support state inference for versions/commands that don't provide a 'State' field in JSON
    let mut status = if dev.state.contains("Attached") {
        "Attached"
    } else if dev.state.contains("Shared") {
        "Shared"
    } else {
        "NotShared"
    };

    if status == "NotShared" {
        if dev.stub_instance_id.is_some() || dev.client_ip_address.is_some() {
            status = "Attached";
        } else if dev.persisted_guid.is_some() {
            status = "Shared";
        }
    }

    // Support extracting VID/PID from InstanceId if they are missing (common in some JSON versions)
    let mut vid = dev.vid.clone();
    let mut pid = dev.pid.clone();
    if (vid.is_none() || pid.is_none()) && full_instance_id.contains("VID_") {
        if let Some(v_start) = full_instance_id.find("VID_") {
            if full_instance_id.len() >= v_start + 8 {
                vid = Some(full_instance_id[v_start + 4..v_start + 8].to_lowercase());
            }
        }
        if let Some(p_start) = full_instance_id.find("PID_") {
            if full_instance_id.len() >= p_start + 8 {
                pid = Some(full_instance_id[p_start + 4..p_start + 8].to_lowercase());
            }
        }
    }

    // Determine if auto-attach is enabled in our configuration
    // Use Bus ID for exact mapping as the user interacts with specific instances
    let vid_pid = format!(
        "{}:{}",
        vid.as_deref().unwrap_or(""),
        pid.as_deref().unwrap_or("")
    );
    let is_in_config = auto_attach_list
        .iter()
        .any(|a| a.bus_id == dev.bus_id.as_deref().unwrap_or_default());

    // Auto-attach only works if the device is currently Shared or Attached.
    // If it's NotShared, the 'Auto' tag shouldn't be effectively 'active'.
    let is_auto = is_in_config && status != "NotShared";

    UsbDevice {
        bus_id: dev.bus_id.clone().unwrap_or_default().into(),
        description: {
            let desc = dev.description.clone().unwrap_or_default();
            let chars: Vec<char> = desc.chars().collect();
            if chars.len() > 80 {
                format!("{}...", chars[..80].iter().collect::<String>()).into()
            } else {
                desc.into()
            }
        },
        instance_id: dev.instance_id.clone().unwrap_or_default().into(),
        sn: sn_part.into(),
        is_real_sn,
        persisted_id: persisted_guid.into(),
        client_ip: dev.client_ip_address.clone().unwrap_or_default().into(),
        stub_id: dev.stub_instance_id.clone().unwrap_or_default().into(),
        is_forced: dev.is_forced,
        vid_pid: vid_pid.into(),
        auto_attach: is_auto,
        status: status.into(),
        is_attached: status == "Attached",
        attached_distro: slint::SharedString::default(),
    }
}

/// Helper: run an async USB operation on a background thread, then update UI.
/// This prevents the UI thread from freezing during external command execution.
fn spawn_usb_task<F, Fut>(ah: slint::Weak<AppWindow>, task_fn: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), String>> + Send + 'static,
{
    tokio::spawn(async move {
        // Run the async task
        let result = task_fn().await;

        let ah_inner = ah.clone();
        let _ = slint::invoke_from_event_loop(move || {
            match result {
                Ok(()) => {
                    if let Some(app) = ah_inner.upgrade() {
                        app.invoke_refresh_usb(true);
                    }
                }
                Err(e) => {
                    // Check for cancellation:
                    // 1. Windows HRESULT 0x800704C7 (The operation was canceled by the user)
                    // 2. Generic "cancel" string from underlying tools/OS
                    let is_canceled =
                        e.contains("0x800704C7") || e.to_lowercase().contains("cancel");

                    if is_canceled {
                        info!("USB operation canceled by user: {}", e);
                        if let Some(app) = ah_inner.upgrade() {
                            app.set_usb_loading(false);
                        }
                    } else if e == "no_wsl2_running" {
                        // Special case: no WSL 2 distro is running.
                        // We show a message dialog instead of turning the whole view into an error state.
                        if let Some(app) = ah_inner.upgrade() {
                            app.set_usb_loading(false);
                            // Set common message dialog properties
                            let msg = crate::ui::data::get_i18n_text("usb.no_wsl2_running");
                            app.set_current_message(msg.into());
                            app.set_show_message_dialog(true);
                        }
                    } else {
                        error!("USB operation failed: {}", e);
                        if let Some(app) = ah_inner.upgrade() {
                            app.set_usb_loading(false);
                            // Try to translate the error if it looks like a key, otherwise show raw
                            let translated = if e.contains('.') || e == "not_found" {
                                crate::ui::data::get_i18n_text(&format!("usb.{}", e))
                            } else {
                                e.clone()
                            };
                            app.set_usb_error(translated.into());
                        }
                    }
                }
            }
        });
    });
}

pub fn setup(app: &AppWindow, app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    // Refresh USB device list — runs usbipd on background thread
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_refresh_usb(move |is_manual| {
        let reason = if is_manual {
            "manual trigger"
        } else {
            "periodic trigger"
        };
        let is_visible = if let Some(app) = ah.upgrade() {
            app.get_is_window_visible()
        } else {
            true
        };

        if !crate::ui::data::should_refresh_usb(reason, is_visible) {
            return;
        }
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        info!("USB refresh requested");

        // 1) Immediately show loading state (non-blocking, UI thread)
        if let Some(app) = ah.upgrade() {
            app.set_usb_loading(true);
            app.set_usb_error(slint::SharedString::default());
        }

        // 2) Run blocking command on tokio thread pool
        let ah_bg = ah.clone();
        tokio::spawn(async move {
            // Get current auto-attach list from config
            let auto_attach_list = {
                let state = as_ptr.lock().await;
                state
                    .config_manager
                    .get_usb_config()
                    .auto_attach_list
                    .clone()
            };
            // First check version
            let version_res = crate::usb::UsbManager::get_version().await;

            let (version, is_outdated) = match version_res {
                Ok(v) => {
                    // Check if version is < 4.0.0
                    let is_old = if let Some(first_char) = v.chars().next() {
                        first_char < '4' && first_char.is_ascii_digit()
                    } else {
                        false
                    };
                    (v, is_old)
                }
                Err(_) => (String::new(), false),
            };

            // If we couldn't even get the version, it's probably not installed
            if version.is_empty() {
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_bg.upgrade() {
                        app.set_usb_loading(false);
                        app.set_usb_error("not_found".into());
                    }
                });
                return;
            }

            // Version is OK, or at least we found the tool
            let result = crate::usb::UsbManager::list_devices().await;

            // 3) Return to UI thread to update state
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_bg.upgrade() {
                    app.set_usbipd_version(version.into());
                    app.set_is_usbipd_outdated(is_outdated);
                    app.set_usb_loading(false);

                    match result {
                        Ok(mut devices) => {
                            // Sort devices by BusId naturally (e.g., 1-1 before 1-10)
                            devices.sort_by(|a, b| {
                                let a_bus = a.bus_id.as_deref().unwrap_or("");
                                let b_bus = b.bus_id.as_deref().unwrap_or("");

                                let a_parts: Vec<&str> =
                                    a_bus.split(|c| c == '-' || c == '.').collect();
                                let b_parts: Vec<&str> =
                                    b_bus.split(|c| c == '-' || c == '.').collect();

                                for (p1, p2) in a_parts.iter().zip(b_parts.iter()) {
                                    let n1 = p1.parse::<u32>();
                                    let n2 = p2.parse::<u32>();

                                    match (n1, n2) {
                                        (Ok(num1), Ok(num2)) => {
                                            if num1 != num2 {
                                                return num1.cmp(&num2);
                                            }
                                        }
                                        _ => {
                                            if p1 != p2 {
                                                return p1.cmp(p2);
                                            }
                                        }
                                    }
                                }
                                a_parts.len().cmp(&b_parts.len())
                            });

                            let slint_devices: Vec<UsbDevice> = devices
                                .iter()
                                .filter(|d| d.bus_id.is_some())
                                .map(|d| to_slint_device(d, &auto_attach_list))
                                .collect();
                            let model = std::rc::Rc::new(slint::VecModel::from(slint_devices));
                            app.set_usb_devices(model.into());
                            info!(
                                "USB device list refreshed: {} devices found (sorted by BusId)",
                                devices.len()
                            );
                        }
                        Err(e) => {
                            error!("Failed to list USB devices: {}", e);
                            if e == "cmd_not_found" {
                                app.set_usb_error("not_found".into());
                            } else {
                                app.set_usb_error(e.into());
                            }
                        }
                    }
                }
            });
        });
    });

    // Bind
    let ah = app_handle.clone();
    app.on_usb_bind(move |bus_id| {
        let bus_id = bus_id.to_string();
        info!("USB bind requested for {}", bus_id);
        if let Some(app) = ah.upgrade() {
            app.set_usb_loading(true);
        }
        spawn_usb_task(ah.clone(), move || async move {
            crate::usb::UsbManager::bind(&bus_id).await
        });
    });

    // Unbind
    let ah = app_handle.clone();
    app.on_usb_unbind(move |bus_id| {
        let bus_id = bus_id.to_string();
        info!("USB unbind requested for {}", bus_id);
        if let Some(app) = ah.upgrade() {
            app.set_usb_loading(true);
        }
        spawn_usb_task(ah.clone(), move || async move {
            crate::usb::UsbManager::unbind(&bus_id).await
        });
    });

    // Attach
    let ah = app_handle.clone();
    app.on_usb_attach(move |bus_id| {
        let bus_id = bus_id.to_string();
        info!("USB attach requested for {}", bus_id);
        if let Some(app) = ah.upgrade() {
            app.set_usb_loading(true);
        }
        spawn_usb_task(ah.clone(), move || async move {
            crate::usb::UsbManager::attach(&bus_id, "").await
        });
    });

    // Detach
    let ah = app_handle.clone();
    app.on_usb_detach(move |bus_id| {
        let bus_id = bus_id.to_string();
        info!("USB detach requested for {}", bus_id);
        if let Some(app) = ah.upgrade() {
            app.set_usb_loading(true);
        }
        spawn_usb_task(ah.clone(), move || async move {
            crate::usb::UsbManager::detach(&bus_id).await
        });
    });

    // Toggle auto-attach (placeholder for now)
    // Toggle auto-attach
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_usb_toggle_auto_attach(move |bus_id| {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let bus_id_str = bus_id.to_string(); // Clone as String for the async task

        info!("USB toggle auto-attach requested for {}", bus_id_str);

        spawn_usb_task(ah.clone(), move || async move {
            // 1. Get device details (VID:PID) asynchronously
            let devices = crate::usb::UsbManager::list_devices().await?;
            let (target_vid_pid, target_distro) = if let Some(dev) = devices.iter().find(|d| d.bus_id.as_deref().unwrap_or("") == bus_id_str) {
                // Ensure we also perform deep VID/PID extraction here (align with list logic)
                let mut vid = dev.vid.clone();
                let mut pid = dev.pid.clone();
                let full_instance_id = dev.instance_id.clone().unwrap_or_default();

                if (vid.is_none() || pid.is_none()) && full_instance_id.contains("VID_") {
                    if let Some(v_start) = full_instance_id.find("VID_") {
                        if full_instance_id.len() >= v_start + 8 {
                            vid = Some(full_instance_id[v_start + 4..v_start + 8].to_lowercase());
                        }
                    }
                    if let Some(p_start) = full_instance_id.find("PID_") {
                        if full_instance_id.len() >= p_start + 8 {
                            pid = Some(full_instance_id[p_start + 4..p_start + 8].to_lowercase());
                        }
                    }
                }

                let vp = format!("{}:{}", vid.as_deref().unwrap_or(""), pid.as_deref().unwrap_or(""));
                (vp, "".to_string())
            } else {
                 return Err(format!("Device {} not found", bus_id_str));
            };

            // 2. Ensure Task Scheduler task exists (for elevated auto-attach background worker)
            let task_exists = crate::network::scheduler::check_task_exists();
            if !task_exists {
                info!("Scheduled task not found, attempting to register with elevation before enabling auto-attach");
                // Attempt to register (will trigger UAC)
                if let Err(e) = crate::network::scheduler::register_task_with_elevation() {
                    error!("Failed to register scheduled task: {}", e);
                    // We continue anyway so the preference is still saved even if task creation fails/cancels
                }

                // Update UI state regardless of result to reflect current reality
                let exists_now = crate::network::scheduler::check_task_exists();
                let ah_ui = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_ui.upgrade() {
                        app.set_network_is_helper_installed(exists_now);
                    }
                });
            }

            // 3. Update config (requires lock)
            {
               let mut state = as_ptr.lock().await;
               let _ = state.config_manager.toggle_usb_auto_attach(&bus_id_str, &target_vid_pid, &target_distro);
            }

            // 3. Trigger refresh on UI thread
            if let Some(app) = ah.upgrade() {
                 app.invoke_refresh_usb(true);
            }
            Ok(())
        });
    });

    // Copy Info
    let ah = app_handle.clone();
    app.on_usb_copy_info(move |device| {
        let json_data = json!({
            "BusId": device.bus_id.as_str(),
            "ClientIPAddress": if device.client_ip.is_empty() { serde_json::Value::Null } else { json!(device.client_ip.as_str()) },
            "Description": device.description.as_str(),
            "InstanceId": device.instance_id.as_str(),
            "IsForced": device.is_forced,
            "PersistedGuid": if device.persisted_id.is_empty() { serde_json::Value::Null } else { json!(device.persisted_id.as_str()) },
            "StubInstanceId": if device.stub_id.is_empty() { serde_json::Value::Null } else { json!(device.stub_id.as_str()) },
        });

        let json_str = serde_json::to_string_pretty(&json_data).unwrap_or_default();
        match copy_to_clipboard(&json_str) {
            Ok(_) => {
                if let Some(app) = ah.upgrade() {
                    let msg = app.global::<AppI18n>().invoke_t("usb.copy_success".into(), slint::ModelRc::from(std::rc::Rc::new(slint::VecModel::from(vec![device.bus_id.clone()]))));
                    app.set_task_status_text(msg);
                    app.set_task_status_visible(true);

                    // Auto-hide toast after 3 seconds
                    let ah_inner = ah.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah_inner.upgrade() {
                                app.set_task_status_visible(false);
                            }
                        });
                    });
                }
            }
            Err(e) => {
                error!("Failed to copy USB info to clipboard: {}", e);
            }
        }
    });
}
