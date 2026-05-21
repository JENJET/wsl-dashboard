use crate::ui::handlers::network::utils::show_toast;
use crate::utils::system::CREATE_NO_WINDOW;
use crate::{AppI18n, AppState, AppWindow, Theme, config, i18n};
use slint::{ComponentHandle, Model};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

pub fn setup(app: &AppWindow, app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_save_settings(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let _ = slint::spawn_local(async move {
            if let Some(app) = ah.upgrade() {
                let ui_language = app.get_ui_language().to_string();
                let distro_location = app.get_distro_location().to_string();
                let logs_location = app.get_logs_location().to_string();
                let auto_shutdown = app.get_auto_shutdown();
                let tray_autostart = app.get_tray_autostart();
                let tray_start_minimized = app.get_tray_start_minimized();
                let tray_close_to_tray = app.get_tray_close_to_tray();
                let log_level = app.get_log_level() as u8;
                let log_days = app.get_log_days() as u8;
                let check_update = app.get_check_update_interval() as u8;
                let system_color = app.get_system_color();

                let sidebar_toggle = app.get_sidebar_toggle();
                let sidebar_add = app.get_sidebar_add();
                let sidebar_wsl_manage = app.get_sidebar_wsl_manage();
                let sidebar_usb = app.get_sidebar_usb();
                let sidebar_network = app.get_sidebar_network();
                let sidebar_about = app.get_sidebar_about();

                let mut state = as_ptr.lock().await;

                let sidebar_config = config::SidebarConfig {
                    toggle: sidebar_toggle,
                    add: sidebar_add,
                    wsl_manage: sidebar_wsl_manage,
                    usb: sidebar_usb,
                    network: sidebar_network,
                    about: sidebar_about,
                };
                if let Err(e) = state.config_manager.update_sidebar_settings(sidebar_config) {
                    error!("Failed to save sidebar settings: {}", e);
                }

                // Apply Dashboard autostart setting to Windows
                if let Err(e) = crate::app::autostart::set_dashboard_autostart(tray_autostart, tray_start_minimized).await {
                    error!("Failed to apply dashboard autostart: {}", e);
                }

                // Update tray settings in config
                let tray_settings = config::TraySettings {
                    autostart: tray_autostart,
                    start_minimized: tray_start_minimized,
                    close_to_tray: tray_close_to_tray,
                };
                if let Err(e) = state.config_manager.update_tray_settings(tray_settings) {
                    error!("Failed to save tray settings: {}", e);
                }

                let temp_location = state.config_manager.get_settings().temp_location.clone();
                let current_logs_location = state.config_manager.get_settings().logs_location.clone();

                // If log path or level changes, update logging system
                if let Some(ls) = state.logging_system.as_mut() {
                    if current_logs_location != logs_location {
                        ls.update_path(&logs_location);
                    }
                    ls.update_level(log_level);
                }

                // Update i18n
                let system_lang = state.config_manager.get_config().system.system_language.clone();
                let old_lang = state.config_manager.get_settings().ui_language.clone();
                let lang_to_load = if ui_language == "auto" {
                    &system_lang
                } else {
                    &ui_language
                };
                i18n::load_resources(lang_to_load);
                app.global::<AppI18n>().set_is_rtl(i18n::is_rtl(lang_to_load));
                app.global::<AppI18n>().set_locale_code(i18n::current_lang().into());
                app.global::<AppI18n>().set_version(app.global::<AppI18n>().get_version() + 1);
                crate::ui::data::refresh_localized_strings(&app);

                // Update font based on new language
                let font_family = if crate::app::is_chinese_lang(lang_to_load) {
                    crate::app::FONT_ZH
                } else {
                    crate::app::FONT_EN_FALLBACK
                };
                app.global::<Theme>().set_default_font(font_family.into());

                // Re-initialize tray if language changed to update menu text
                if old_lang != ui_language {
                    info!("Language changed from '{}' to '{}', triggering system tray re-initialization...", old_lang, ui_language);
                    let ah_tray = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_tray.upgrade() {
                            app.invoke_reinit_tray();
                        }
                    });
                }


                let terminal_presets = state.config_manager.get_settings().terminal_presets.clone();
                let terminal_user_presets = state.config_manager.get_settings().terminal_user_presets.clone();
                let terminal_emulator = {
                    let idx = app.get_terminal_emulator_index() as usize;
                    let options = app.get_terminal_emulator_options();
                    if idx < options.row_count() as usize {
                        options.row_data(idx).map(|s| s.to_string()).unwrap_or_default()
                    } else {
                        state.config_manager.get_settings().terminal_emulator.clone()
                    }
                };

                if !terminal_emulator.is_empty() {
                    let builtin = crate::wsl::terminal::get_builtin_presets_map();
                    let all_presets = crate::wsl::terminal::resolve_presets(
                        builtin,
                        &terminal_presets,
                        &terminal_user_presets,
                    );
                    if let Some(preset) = all_presets.get(&terminal_emulator) {
                        if let Err(_) = crate::wsl::terminal::validate_preset(preset) {
                            let error_msg = i18n::tr(
                                "dialog.terminal.validation_file_not_found",
                                &[preset.path.clone()],
                            );
                            drop(state);
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(app) = ah.upgrade() {
                                    app.set_current_message(error_msg.into());
                                    app.set_show_message_dialog(true);
                                }
                            });
                            return;
                        }
                    } else {
                        let error_msg = i18n::tr(
                            "dialog.terminal.preset_error",
                            &[terminal_emulator.clone()],
                        );
                        drop(state);
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah.upgrade() {
                                app.set_current_message(error_msg.into());
                                app.set_show_message_dialog(true);
                            }
                        });
                        return;
                    }
                }

                let user_settings = config::UserSettings {
                    modify_time: chrono::Utc::now().timestamp_millis().to_string(),
                    distro_location,
                    logs_location: logs_location.clone(),
                    temp_location,
                    ui_language,
                    auto_shutdown,
                    dark_mode: app.global::<Theme>().get_dark_mode(),
                    log_level,
                    log_days,
                    check_update,
                    check_time: state.config_manager.get_settings().check_time.clone(),
                    sidebar_collapsed: app.get_sidebar_collapsed(),
                    system_color,
                    terminal_emulator,
                    terminal_presets,
                    terminal_user_presets,
                };

                // Dynamic ThemeWatcher switching
                let old_system_color = state.config_manager.get_settings().system_color;
                if old_system_color != system_color {
                    if system_color {
                        match crate::utils::theme::ThemeWatcher::new(ah.clone()) {
                            Ok(watcher) => {
                                let theme = crate::utils::theme::ThemeWatcher::get_current_theme();
                                app.global::<crate::Theme>().set_dark_mode(theme == crate::utils::theme::Theme::Dark);
                                state.theme_watcher = Some(watcher);
                                info!("ThemeWatcher enabled via settings.");
                            }
                            Err(e) => {
                                error!("Failed to enable ThemeWatcher: {}", e);
                            }
                        }
                    } else {
                        state.theme_watcher = None;
                        info!("ThemeWatcher disabled via settings.");
                    }
                }

                match state.config_manager.update_settings(user_settings) {
                    Ok(_) => {
                        drop(state);
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah.upgrade() {
                                // Sync saved terminal index so next settings visit uses saved value
                                app.set_saved_terminal_emulator_index(app.get_terminal_emulator_index());
                                // Translate message if possible, or just keep english for now as it's dynamic
                                // But better to use a key if we had one "settings.saved_success"
                                app.set_current_message(i18n::t("settings.saved_success").into());
                                app.set_show_message_dialog(true);
                            }
                        });
                    }
                    Err(e) => {
                        let error_msg = i18n::tr("settings.saved_failed", &[e.to_string()]);
                        drop(state);
                        error!("{}", error_msg);
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah.upgrade() {
                                app.set_current_message(error_msg.into());
                                app.set_show_message_dialog(true);
                            }
                        });
                    }
                }
            }
        });
    });

    let ah = app_handle.clone();
    app.on_select_distro_folder(move || {
        if let Some(path) = rfd::FileDialog::new()
            .set_title(i18n::t("settings.select_distro_dir"))
            .pick_folder()
        {
            if let Some(app) = ah.upgrade() {
                app.set_distro_location(path.display().to_string().into());
            }
        }
    });

    let ah = app_handle.clone();
    app.on_select_logs_folder(move || {
        if let Some(path) = rfd::FileDialog::new()
            .set_title(i18n::t("settings.select_log_dir"))
            .pick_folder()
        {
            if let Some(app) = ah.upgrade() {
                app.set_logs_location(path.display().to_string().into());
            }
        }
    });

    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_toggle_theme(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let _ = slint::spawn_local(async move {
            if let Some(app) = ah.upgrade() {
                let dark_mode = app.global::<Theme>().get_dark_mode();
                let mut state = as_ptr.lock().await;
                let mut settings = state.config_manager.get_settings().clone();
                settings.dark_mode = dark_mode;
                if let Err(e) = state.config_manager.update_settings(settings) {
                    error!("Failed to save color mode: {}", e);
                } else {
                    info!(
                        "Color mode saved: {}",
                        if dark_mode { "Dark" } else { "Light" }
                    );
                }
            }
        });
    });

    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_open_wsl_settings(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let _ = slint::spawn_local(async move {
            let state = as_ptr.lock().await;
            let executor = state.wsl_dashboard.executor().clone();
            drop(state);

            let show_upgrade_prompt = |app: slint::Weak<AppWindow>| {
                if let Some(app) = app.upgrade() {
                    app.set_current_message(i18n::t("settings.wsl2_required").into());
                    app.set_current_message_link(i18n::t("settings.update_wsl").into());
                    app.set_current_message_url(
                        "https://github.com/microsoft/WSL/releases/latest".into(),
                    );
                    app.set_show_message_dialog(true);
                }
            };

            // 1. Check if it's the Store version (which supports WSL Settings)
            // If wsl --version fails, it's likely the Inbox version or an old version
            let version_check = executor.execute_command(&["--version"]).await;
            if !version_check.success {
                show_upgrade_prompt(ah);
                return;
            }

            // 2. Discover wslsettings.exe path
            let rel_path = "Program Files\\WSL\\wslsettings\\wslsettings.exe";
            let mut exe_path = std::path::PathBuf::from(format!("C:\\{}", rel_path));
            let mut found = exe_path.exists();

            if !found {
                // Try SystemDrive if not C:
                if let Ok(system_drive) = std::env::var("SystemDrive") {
                    if system_drive.to_uppercase() != "C:" {
                        let alt_path =
                            std::path::PathBuf::from(format!("{}\\{}", system_drive, rel_path));
                        if alt_path.exists() {
                            exe_path = alt_path;
                            found = true;
                        }
                    }
                }
            }

            if !found {
                // Exhaustive search on other drive letters
                for drive in b'C'..=b'Z' {
                    let drive_str = format!("{}:", drive as char);
                    let alt_path = std::path::PathBuf::from(format!("{}\\{}", drive_str, rel_path));
                    if alt_path.exists() {
                        exe_path = alt_path;
                        found = true;
                        break;
                    }
                }
            }

            if found {
                let mut cmd = std::process::Command::new(exe_path);
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    cmd.creation_flags(CREATE_NO_WINDOW);
                }
                let _ = cmd.spawn().map_err(|e| {
                    error!("Failed to launch WSL settings: {}", e);
                });
            } else {
                // If wslsettings.exe is not found even on multiple drives,
                // it's almost certainly because the WSL version is < 2.3.0
                show_upgrade_prompt(ah);
            }
        });
    });

    let ah = app_handle.clone();
    app.on_terminal_path_browse(move || {
        if let Some(path) = rfd::FileDialog::new()
            .set_title(i18n::t("settings.select_terminal_exe"))
            .add_filter("Executable", &["exe"])
            .pick_file()
        {
            if let Some(app) = ah.upgrade() {
                app.set_terminal_custom_path(path.display().to_string().into());
            }
        }
    });

    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_terminal_refresh_terminals(move || {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let _ = slint::spawn_local(async move {
            crate::wsl::terminal::invalidate_caches();
            let state = as_ptr.lock().await;
            let settings = state.config_manager.get_settings().clone();
            drop(state);
            if let Some(app) = ah.upgrade() {
                crate::ui::data::refresh_terminal_emulator_options(&app, &settings);
                show_toast(app.as_weak(), i18n::t("toast.operation_success"));
            }
        });
    });

    // User-defined terminal preset management
    let ah = app_handle.clone();
    app.on_add_user_preset(move || {
        if let Some(app) = ah.upgrade() {
            app.set_terminal_user_preset_form_title(i18n::t("dialog.terminal.add_preset").into());
            app.set_terminal_user_preset_form_name("".into());
            app.set_terminal_user_preset_form_path("".into());
            app.set_terminal_user_preset_form_args("wsl -d {distro} --cd {dir}".into());
            app.set_terminal_user_preset_original_name("".into());
            app.set_terminal_user_preset_form_error("".into());
            app.set_terminal_show_user_preset_form(true);
        }
    });

    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_edit_user_preset(move |name: slint::SharedString| {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let name = name.to_string();
        let _ = slint::spawn_local(async move {
            let state = as_ptr.lock().await;
            let settings = state.config_manager.get_settings().clone();
            drop(state);
            let preset_opt = settings.terminal_user_presets.get(&name).cloned();
            if let Some(preset) = preset_opt {
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah.upgrade() {
                        app.set_terminal_user_preset_form_title(
                            i18n::t("dialog.terminal.edit_preset").into(),
                        );
                        app.set_terminal_user_preset_form_name(name.clone().into());
                        app.set_terminal_user_preset_form_path(preset.path.clone().into());
                        app.set_terminal_user_preset_form_args(preset.args.clone().into());
                        app.set_terminal_user_preset_original_name(name.into());
                        app.set_terminal_user_preset_form_error("".into());
                        app.set_terminal_show_user_preset_form(true);
                    }
                });
            }
        });
    });

    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_delete_user_preset(move |name: slint::SharedString| {
        let ah = ah.clone();
        let as_ptr = as_ptr.clone();
        let name = name.to_string();
        let _ = slint::spawn_local(async move {
            let mut state = as_ptr.lock().await;
            let mut settings = state.config_manager.get_settings().clone();
            settings.terminal_user_presets.remove(&name);
            let was_global_reset = settings.terminal_emulator == name;
            if was_global_reset {
                settings.terminal_emulator = crate::wsl::terminal::BuiltinTerminal::Cmd
                    .as_str()
                    .to_string();
            }
            let user_names: Vec<String> = settings.terminal_user_presets.keys().cloned().collect();
            let updated_settings = settings.clone();
            if let Err(e) = state.config_manager.update_settings(settings) {
                error!("Failed to delete user preset: {}", e);
            }

            // Reset per-instance terminal emulator if it was set to the deleted preset
            let instances_path = crate::config::ConfigManager::get_instances_path();
            let mut container = crate::config::instances::load_instances(&instances_path);
            let mut instances_modified = false;
            for (_, cfg) in container.instances.iter_mut() {
                if cfg.terminal_emulator == name {
                    cfg.terminal_emulator.clear();
                    instances_modified = true;
                }
            }
            if instances_modified {
                container.common.modify_time = chrono::Utc::now().timestamp_millis().to_string();
                let _ =
                    crate::config::instances::save_instances_to_disk(&instances_path, &container);
            }

            drop(state);
            let ah_for_refresh = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah.upgrade() {
                    crate::ui::data::refresh_terminal_emulator_options(&app, &updated_settings);
                    let mut sorted = user_names;
                    sorted.sort();
                    let model: slint::ModelRc<slint::SharedString> =
                        slint::ModelRc::new(slint::VecModel::from(
                            sorted
                                .iter()
                                .map(|s| slint::SharedString::from(s.as_str()))
                                .collect::<Vec<_>>(),
                        ));
                    app.set_terminal_user_preset_names(model);
                    app.set_terminal_show_user_preset_form(false);
                }
            });
            if was_global_reset || instances_modified {
                crate::ui::data::refresh_distros_ui(ah_for_refresh, as_ptr.clone()).await;
            }
        });
    });

    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_save_user_preset(
        move |old_name: slint::SharedString,
              name: slint::SharedString,
              path: slint::SharedString,
              args: slint::SharedString| {
            let ah = ah.clone();
            let as_ptr = as_ptr.clone();
            let old_name = old_name.to_string();
            let name = name.to_string();
            let path = path.to_string();
            let args = args.to_string();

            if name.is_empty() {
                if let Some(app) = ah.upgrade() {
                    app.set_terminal_user_preset_form_error(
                        i18n::t("dialog.terminal.preset_name_empty").into(),
                    );
                }
                return;
            }
            if path.is_empty() {
                if let Some(app) = ah.upgrade() {
                    app.set_terminal_user_preset_form_error(
                        i18n::t("dialog.terminal.preset_path_empty").into(),
                    );
                }
                return;
            }
            if args.is_empty() {
                if let Some(app) = ah.upgrade() {
                    app.set_terminal_user_preset_form_error(
                        i18n::t("dialog.terminal.preset_args_empty").into(),
                    );
                }
                return;
            }

            // Validate executable path exists and is an .exe file
            if !path.to_lowercase().ends_with(".exe") {
                if let Some(app) = ah.upgrade() {
                    app.set_terminal_user_preset_form_error(
                        i18n::t("dialog.terminal.validation_not_exe").into(),
                    );
                }
                return;
            }
            if !std::path::Path::new(&path).exists() {
                if let Some(app) = ah.upgrade() {
                    app.set_terminal_user_preset_form_error(
                        i18n::tr("dialog.terminal.validation_file_not_found", &[path]).into(),
                    );
                }
                return;
            }

            let _ = slint::spawn_local(async move {
                let mut state = as_ptr.lock().await;
                let mut settings = state.config_manager.get_settings().clone();

                if !old_name.is_empty() && old_name != name {
                    settings.terminal_user_presets.remove(&old_name);
                }

                if settings.terminal_user_presets.keys().any(|k| {
                    k.eq_ignore_ascii_case(&name) && (old_name.is_empty() || *k != old_name)
                }) {
                    drop(state);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah.upgrade() {
                            app.set_terminal_user_preset_form_error(
                                i18n::t("dialog.terminal.preset_name_taken").into(),
                            );
                        }
                    });
                    return;
                }

                settings.terminal_user_presets.insert(
                    name.clone(),
                    config::TerminalPreset {
                        path: path.clone(),
                        args: args.clone(),
                    },
                );

                let user_names: Vec<String> =
                    settings.terminal_user_presets.keys().cloned().collect();
                let updated_settings = settings.clone();

                if let Err(e) = state.config_manager.update_settings(settings) {
                    error!("Failed to save user preset: {}", e);
                }
                drop(state);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah.upgrade() {
                        crate::ui::data::refresh_terminal_emulator_options(&app, &updated_settings);
                        let mut sorted = user_names;
                        sorted.sort();
                        let model: slint::ModelRc<slint::SharedString> =
                            slint::ModelRc::new(slint::VecModel::from(
                                sorted
                                    .iter()
                                    .map(|s| slint::SharedString::from(s.as_str()))
                                    .collect::<Vec<_>>(),
                            ));
                        app.set_terminal_user_preset_names(model);
                        app.set_terminal_show_user_preset_form(false);
                    }
                });
            });
        },
    );

    let ah = app_handle.clone();
    app.on_cancel_user_preset_form(move || {
        if let Some(app) = ah.upgrade() {
            app.set_terminal_show_user_preset_form(false);
            app.set_terminal_user_preset_form_error("".into());
        }
    });

    let ah = app_handle.clone();
    app.on_user_preset_path_browse(move || {
        if let Some(path) = rfd::FileDialog::new()
            .set_title(i18n::t("settings.select_terminal_exe"))
            .add_filter("Executable", &["exe"])
            .pick_file()
        {
            if let Some(app) = ah.upgrade() {
                app.set_terminal_user_preset_form_path(path.display().to_string().into());
            }
        }
    });
}
