#![windows_subsystem = "windows"]

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

use slint::{ComponentHandle, Model};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

// Define application modules
mod app;
mod config;
mod i18n;
mod network;
mod ui;
mod usb;
mod utils;
mod wsl;

// Import Slint UI components
slint::include_modules!();

use app::{APP_ID, APP_NAME, AppState, GITHUB_ISSUES, GITHUB_URL, WSL_INIT_SCRIPT};
use ui::data::refresh_data;
use ui::handlers;

#[tokio::main]
async fn main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    // Check command line arguments
    let args: Vec<String> = std::env::args().collect();

    #[cfg(all(debug_assertions, windows))]
    {
        // Try to attach to parent console in debug mode so `cargo run` logs are visible.
        // Skip attaching if we are specifically running the scheduler.
        if !args.iter().any(|a| a == "/scheduler") {
            unsafe {
                use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
                let _ = AttachConsole(ATTACH_PARENT_PROCESS);
            }
        }
    }

    // 1. Initialize configuration manager first to get log path and settings
    let config_manager = config::ConfigManager::new().await;
    let settings = config_manager.get_settings().clone();
    let tray_settings = config_manager.get_tray_settings().clone();

    // 2. Load i18n based on settings
    let system_language = config_manager.get_config().system.system_language.clone();
    let lang = if settings.ui_language == "auto" {
        &config_manager.get_config().system.system_language
    } else {
        &settings.ui_language
    };
    i18n::load_resources(lang);

    // 3. Set up tracing logs EARLY to capture potential /scheduler or startup errors
    let initial_logs_location = settings.logs_location.clone();
    let log_level = settings.log_level;
    let timezone = config_manager.get_config().system.timezone.clone();
    let logging_system = utils::logging::init_logging(&initial_logs_location, log_level, &timezone);

    // 4. Initial check for network sync command before single instance or other UI loads
    if let Some(pos) = args.iter().position(|a| a == "/scheduler") {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }

        info!(">>> [START] Network sync command detected via /scheduler <<<");

        // 4.0 Cleanup legacy startup scripts (vbs) asynchronously with timeout protection
        crate::utils::system::cleanup_legacy_vbs_startup();

        // 4.1 Load all necessary configurations
        let instances_path = config::ConfigManager::get_instances_path();
        let container = config::instances::load_instances(&instances_path);
        let net_config = config_manager.get_network_config();
        let rules = &net_config.port_proxies;

        // 4.2 Identify target distros for synchronization and auto-start
        let mut target_distros: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        if pos + 1 < args.len() && !args[pos + 1].starts_with('/') {
            target_distros.insert(args[pos + 1].clone());
        } else {
            // Source 1: Distros with port proxy rules
            for r in rules {
                target_distros.insert(r.distro_name.clone());
            }

            // Source 2: Distros with auto-startup enabled
            for (name, inst) in &container.instances {
                if inst.auto_startup {
                    target_distros.insert(name.clone());
                }
            }

            // Source 3: Distros with USB auto-attach configurations
            let usb_config = config_manager.get_usb_config();
            for device in &usb_config.auto_attach_list {
                target_distros.insert(device.distribution.clone());
            }
        }

        // 4.3 Background startup for target distros (if configured for auto-startup)
        let mut distros_spawned = 0;
        for name in &target_distros {
            if let Some(inst_config) = container.instances.get(name) {
                if inst_config.auto_startup {
                    info!(
                        "Distro '{}' marked for auto-start. Spawning background init script...",
                        name
                    );
                    #[cfg(windows)]
                    {
                        use std::os::windows::process::CommandExt;
                        use std::process::Command;
                        const CREATE_NO_WINDOW: u32 = 0x08000000;

                        let _ = Command::new("wsl")
                            .args(&["-d", name, "-u", "root", WSL_INIT_SCRIPT, "start"])
                            .creation_flags(CREATE_NO_WINDOW)
                            .spawn();
                        distros_spawned += 1;
                    }
                }
            }
        }

        // 4.4 Wait for network interfaces to stabilize if any distros were spawned
        if distros_spawned > 0 {
            info!(
                "Waiting 5 seconds for {} spawned distros to stabilize network interfaces...",
                distros_spawned
            );
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        } else {
            info!("No new distros were spawned. Proceeding with existing running instances.");
        }

        // 4.5 Execute synchronization for each target distro
        for name in target_distros {
            let distro_rules: Vec<_> = rules
                .iter()
                .filter(|r| r.distro_name == name)
                .cloned()
                .collect();
            if distro_rules.is_empty() {
                info!("No rules found for distro '{}', skipping.", name);
                continue;
            }

            info!(
                ">>> Executing elevated sync for '{}' (Total Rules: {})",
                name,
                distro_rules.len()
            );
            // sync_port_proxies has internal 10-retry logic for IP fetching
            if let Err(e) = network::port_proxy::sync_port_proxies(&name, &distro_rules) {
                error!("Sync FAILED for '{}': {}", name, e);
            } else {
                info!("Sync SUCCESS for '{}'.", name);
            }
        }

        // 4.6 Auto-attach USB devices if configured
        info!(">>> [START] USB auto-attach synchronization <<<");
        let usb_config = config_manager.get_usb_config().clone();

        if !usb_config.auto_attach_list.is_empty() {
            // Check if any WSL 2 instance is running (required for usbipd attach)
            let is_any_running = {
                let mut cmd = std::process::Command::new("wsl");
                cmd.args(["-l", "-v"]);
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x08000000;
                    cmd.creation_flags(CREATE_NO_WINDOW);
                }
                cmd.env("WSL_UTF8", "1");

                match cmd.output() {
                    Ok(out) => {
                        let stdout = crate::wsl::decoder::decode_output(&out.stdout);
                        stdout
                            .lines()
                            .skip(1) // Skip header
                            .any(|line| {
                                let lower = line.to_lowercase();
                                let parts: Vec<&str> = line.split_whitespace().collect();
                                // Must be Running AND Version 2
                                lower.contains("running") && parts.iter().any(|&p| p == "2")
                            })
                    }
                    Err(_) => false,
                }
            };

            if is_any_running {
                info!(
                    "Found running WSL 2 instance. Processing {} auto-attach device(s)...",
                    usb_config.auto_attach_list.len()
                );
                for device in &usb_config.auto_attach_list {
                    info!(
                        "Auto-attaching USB device: BusId={}, VidPid={}, TargetDistro='{}'",
                        device.bus_id, device.vid_pid, device.distribution
                    );

                    // UsbManager::attach internally performs 'usbipd bind' then 'usbipd attach'
                    match crate::usb::UsbManager::attach(&device.bus_id, &device.distribution).await
                    {
                        Ok(_) => info!("SUCCESS: USB device {} attached.", device.bus_id),
                        Err(e) => {
                            error!("FAILED to auto-attach USB device {}: {}", device.bus_id, e);
                        }
                    }
                }
            } else {
                info!("No running WSL 2 instance found. Skipping USB auto-attach.");
            }
        } else {
            info!("No USB devices configured for auto-attach.");
        }
        info!(">>> [FINISH] USB auto-attach synchronization completed. <<<");

        info!(">>> [FINISH] All scheduled network tasks completed. <<<");
        // Ensure logs are flushed before exit
        drop(logging_system);
        return; // Exit short-lived cli process
    }

    // 5. Silent mode detection
    let is_silent_mode = args.iter().any(|arg| arg.eq_ignore_ascii_case("/silent"));

    // 6. Single Instance check (after log initialization to capture errors)
    let instance = app::single_instance::SingleInstance::new("wsldashboard-v0.3-lock");
    if !instance.is_single() {
        if !is_silent_mode {
            if app::single_instance::try_activate_existing_instance() {
                info!("Activated existing instance, exiting...");
            } else {
                eprintln!("Another instance is already running. Exiting.");
            }
        } else {
            eprintln!("Another instance is already running (silent mode). Exiting.");
        }
        return;
    }

    // 7. Cleanup expired logs
    utils::logging::cleanup_expired_logs(&initial_logs_location, settings.log_days);

    info!("Starting {} (ID: {})...", APP_NAME, APP_ID);

    if is_silent_mode {
        info!("Silent mode detected via /silent parameter");
    }

    // 8. Path automatic repair: update registry if the exe has been moved
    app::autostart::repair_autostart_path(tray_settings.autostart, tray_settings.start_minimized)
        .await;

    // Create application state
    let app_state = Arc::new(Mutex::new(AppState::new(
        config_manager,
        logging_system,
        is_silent_mode,
    )));

    // Create Slint application
    let app = AppWindow::new().expect("Failed to create app");
    app.set_system_language(system_language.into());

    // Register i18n callback
    app.global::<AppI18n>().on_t(|key, args| {
        let args_vec: Vec<String> = args
            .iter()
            .map(|s: slint::SharedString| s.to_string())
            .collect();
        i18n::tr(&key, &args_vec).into()
    });

    // Initialize locale code and RTL status
    let current_lang = i18n::current_lang();
    app.global::<AppI18n>().set_locale_code(current_lang.into());

    // Trigger initial evaluation of all i18n properties
    app.global::<AppI18n>().set_version(1);

    // Set version number and URLs - homepage and issues always use GITHUB_URL
    app.global::<AppInfo>()
        .set_version(env!("CARGO_PKG_VERSION").into());
    app.global::<AppInfo>().set_homepage(GITHUB_URL.into());
    app.global::<AppInfo>()
        .set_issues_url(format!("{}{}", GITHUB_URL, GITHUB_ISSUES).into());

    // 9. Initialize system tray
    if let Err(e) = app::tray::SystemTray::initialize(app.as_weak(), !is_silent_mode) {
        error!("Failed to initialize system tray: {}", e);
    }

    app.on_reinit_tray({
        let ah = app.as_weak();
        move || {
            let current_visible = if let Some(app) = ah.upgrade() {
                app.get_is_window_visible()
            } else {
                false
            };
            info!(
                "Re-initializing tray, current visibility: {}",
                current_visible
            );
            if let Err(e) = app::tray::SystemTray::initialize(ah.clone(), current_visible) {
                error!("Failed to re-initialize system tray: {}", e);
            }
        }
    });

    // 10. Load settings to UI (crucial for i18n, fonts, and theme)
    ui::data::load_settings_to_ui(&app, &app_state, &settings, &tray_settings).await;

    // 10.1 Initialize System Theme Watcher if enabled
    if settings.system_color {
        match crate::utils::theme::ThemeWatcher::new(app.as_weak()) {
            Ok(watcher) => {
                let theme = crate::utils::theme::ThemeWatcher::get_current_theme();
                app.global::<crate::Theme>()
                    .set_dark_mode(theme == crate::utils::theme::Theme::Dark);

                let mut state = app_state.lock().await;
                state.theme_watcher = Some(watcher);
            }
            Err(e) => {
                error!("Failed to initialize ThemeWatcher: {}", e);
            }
        }
    }

    // 11. Setup handlers
    handlers::setup(&app, app.as_weak(), app_state.clone()).await;

    // 12. Refresh initial data (distros list)
    refresh_data(app.as_weak(), app_state.clone()).await;

    // 13. Start background tasks (check for updates, monitor WSL/USB status)
    app::startup::spawn_check_task(app.as_weak(), app_state.clone());
    app::tasks::spawn_wsl_monitor(app.as_weak(), app_state.clone());
    app::tasks::spawn_resource_monitor(app.as_weak());
    app::tasks::spawn_usb_monitor(app.as_weak());
    app::tasks::spawn_state_listener(app.as_weak(), app_state.clone());

    // Start UI
    app::window::show_and_center(&app, is_silent_mode);

    // 14. Run application event loop with a keep-alive timer to prevent exit when hidden
    // We Box::leak to ensure the timer stays alive as long as the process runs.
    let keep_alive_timer = Box::leak(Box::new(slint::Timer::default()));
    keep_alive_timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_secs(1),
        || {
            // Keep-alive heartbeat
        },
    );

    slint::run_event_loop().expect("Failed to run Slint event loop");

    // 15. Handle cleanup on exit
    app::tasks::handle_app_exit(&app, &app_state).await;
}
