use super::sanitize_instance_name;
use crate::download::FlatDistroEntry;
use crate::ui::data::refresh_installable_distros;
use crate::{AppState, AppWindow, UrlDistroInfo, i18n};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Check if a string is a local file path
fn is_local_path(path: &str) -> bool {
    if path.starts_with("file://") {
        return true;
    }
    if path.starts_with("http://") || path.starts_with("https://") || path.starts_with("ftp://") {
        return false;
    }
    Path::new(path).exists()
}

/// Strip file:// prefix, return the local filesystem path
fn strip_file_protocol(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("file://") {
        rest.to_string()
    } else {
        path.to_string()
    }
}

pub fn setup(app: &AppWindow, app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    // Callback: check if a path is local (used by UI to decide auto-fetch)
    app.on_is_local_url(|path| is_local_path(path.as_str()));

    // Folder selection
    let ah = app_handle.clone();
    app.on_select_folder(move || {
        if let Some(path) = rfd::FileDialog::new()
            .set_title(i18n::t("dialog.select_install_dir"))
            .pick_folder()
        {
            if let Some(app) = ah.upgrade() {
                let path_str = path.display().to_string();
                app.set_new_instance_path(path_str.clone().into());

                let p = std::path::Path::new(&path_str);
                if p.exists() {
                    if let Ok(entries) = std::fs::read_dir(p) {
                        if entries.count() > 0 {
                            app.set_path_error(i18n::t("dialog.dir_not_empty").into());
                        } else {
                            app.set_path_error("".into());
                        }
                    }
                } else {
                    app.set_path_error("".into());
                }
            }
        }
    });

    let ah = app_handle.clone();
    app.on_check_install_path(move |path| {
        if let Some(app) = ah.upgrade() {
            if path.is_empty() {
                app.set_path_error("".into());
                return;
            }
            let p = std::path::Path::new(path.as_str());
            if p.exists() && p.is_dir() {
                if let Ok(entries) = std::fs::read_dir(p) {
                    if entries.count() > 0 {
                        app.set_path_error(i18n::t("dialog.dir_not_empty").into());
                        return;
                    }
                }
            }
            app.set_path_error("".into());
        }
    });

    let ah = app_handle.clone();
    app.on_select_install_file(move |source_idx| {
        let mut dialog = rfd::FileDialog::new().set_title(i18n::t("dialog.select_install_file"));

        dialog = match source_idx {
            0 => dialog.add_filter(
                i18n::t("dialog.archive"),
                &["tar", "tar.gz", "tar.xz", "wsl"],
            ),
            1 => dialog.add_filter(i18n::t("dialog.vhdx"), &["vhdx"]),
            3 => dialog.add_filter(i18n::t("dialog.json"), &["json"]),
            _ => dialog,
        };

        if let Some(path) = dialog.pick_file() {
            if let Some(app) = ah.upgrade() {
                // For URL mode (source_idx 3), set custom_install_url and trigger fetch
                if source_idx == 3 {
                    let path_str = path.display().to_string();
                    app.set_custom_install_url(path_str.into());
                    app.invoke_fetch_url_distros();
                    return;
                }
                app.set_install_file_path(path.display().to_string().into());

                if let Some(name_os) = path.file_name() {
                    let mut full_stem = name_os.to_string_lossy().to_string();

                    // Optimize: Remove specific suffixes first to get clean name
                    if full_stem.ends_with(".tar.gz") {
                        full_stem.truncate(full_stem.len() - 7);
                    } else if full_stem.ends_with(".tar.xz") {
                        full_stem.truncate(full_stem.len() - 7);
                    } else if full_stem.ends_with(".tar") {
                        full_stem.truncate(full_stem.len() - 4);
                    } else if full_stem.ends_with(".wsl") {
                        full_stem.truncate(full_stem.len() - 4);
                    } else if full_stem.ends_with(".vhdx") {
                        full_stem.truncate(full_stem.len() - 5);
                    }
                    // Remove "rootfs" case-insensitively
                    while let Some(idx) = full_stem.to_lowercase().find("rootfs") {
                        full_stem.replace_range(idx..idx + 6, "");
                    }

                    let parts: Vec<&str> = full_stem.split('-').collect();
                    let mut filtered_parts = Vec::new();
                    let stop_keywords = [
                        "wsl", "amd64", "arm64", "x86_64", "with", "docker", "vhdx", "image",
                    ];

                    for part in parts {
                        let lower_part = part.to_lowercase();
                        if stop_keywords.iter().any(|&k| lower_part.contains(k)) {
                            break;
                        }
                        if !part.is_empty() && part != "." {
                            filtered_parts.push(part);
                        }
                    }

                    let suggested_name = if filtered_parts.is_empty() {
                        full_stem
                    } else {
                        filtered_parts.join("-")
                    };

                    let mut sanitized = sanitize_instance_name(&suggested_name);

                    while sanitized.ends_with(|c| c == '-' || c == '_' || c == '.') {
                        sanitized.pop();
                    }

                    app.set_new_instance_name(sanitized.clone().into());

                    // Sync path
                    let distro_location = app.get_distro_location().to_string();
                    let new_path = std::path::Path::new(&distro_location)
                        .join(&sanitized)
                        .to_string_lossy()
                        .to_string();
                    app.set_new_instance_path(new_path.into());
                }
            }
        }
    });

    let ah = app_handle.clone();
    app.on_distro_selected(move |val| {
        if let Some(app) = ah.upgrade() {
            let app_typed: AppWindow = app;
            let installables = app_typed.get_installable_distros();
            let mut internal_id = val.to_string();

            // Try to find the internal ID from the model
            for i in 0..installables.row_count() {
                if let Some(d) = installables.row_data(i) {
                    if d.friendly_name == val {
                        internal_id = d.name.to_string();
                        break;
                    }
                }
            }

            let sanitized = sanitize_instance_name(&internal_id);
            app_typed.set_new_instance_name(sanitized.clone().into());
            app_typed.set_selected_install_distro(internal_id.into()); // Store internal ID

            let distro_location = app_typed.get_distro_location().to_string();
            let new_path = std::path::Path::new(&distro_location)
                .join(&sanitized)
                .to_string_lossy()
                .to_string();
            app_typed.set_new_instance_path(new_path.into());
        }
    });

    // Cache of full distribution entries (both architectures)
    let url_entries_cache: Arc<Mutex<Option<Vec<FlatDistroEntry>>>> = Arc::new(Mutex::new(None));

    // Helper: rebuild UI models from cache filtered by current architecture.
    // Preserves the previous selection if the distro still exists in the filtered list.
    fn rebuild_url_models_from(
        ah: &slint::Weak<AppWindow>,
        cache: &Arc<Mutex<Option<Vec<FlatDistroEntry>>>>,
    ) {
        let ah = ah.clone();
        let cache = cache.clone();
        let _ = slint::spawn_local(async move {
            // Read previous selection before rebuilding
            let prev_selected_name = ah.upgrade().and_then(|a| {
                let list = a.get_url_distro_list();
                let idx = a.get_selected_url_distro_idx();
                if idx >= 0 && (idx as usize) < list.row_count() {
                    list.row_data(idx as usize).map(|d| d.name.to_string())
                } else {
                    None
                }
            });

            let is_arm64 = ah
                .upgrade()
                .map(|a| a.get_url_distro_is_arm64())
                .unwrap_or(false);
            let entries_opt = cache.lock().await;
            let entries = match &*entries_opt {
                Some(e) => e,
                None => return,
            };

            let mut slint_entries = Vec::new();
            let mut name_list: Vec<SharedString> = Vec::new();
            for e in entries {
                let url = if is_arm64 { &e.arm64_url } else { &e.amd64_url };
                let sha256 = if is_arm64 {
                    &e.arm64_sha256
                } else {
                    &e.amd64_sha256
                };
                if url.is_empty() {
                    continue;
                }
                slint_entries.push(UrlDistroInfo {
                    name: e.name.clone().into(),
                    friendly_name: e.friendly_name.clone().into(),
                    url: url.clone().into(),
                    sha256: sha256.clone().into(),
                    arm64_url: e.arm64_url.clone().into(),
                    arm64_sha256: e.arm64_sha256.clone().into(),
                });
                name_list.push(e.friendly_name.clone().into());
            }

            let model = VecModel::from(slint_entries);
            let model_rc = ModelRc::from(Rc::new(model));
            let names_model = VecModel::from(name_list);
            let names_rc = ModelRc::from(Rc::new(names_model));

            if let Some(app) = ah.upgrade() {
                app.set_url_distro_list(model_rc);
                app.set_url_distro_names(names_rc);

                let count = app.get_url_distro_names().row_count();
                if count > 0 {
                    // Try to restore previous selection, fall back to first entry
                    let sel_idx = prev_selected_name
                        .and_then(|prev| {
                            (0..count).find(|&i| {
                                app.get_url_distro_list()
                                    .row_data(i)
                                    .map(|d| d.name == prev)
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(0);

                    app.set_selected_url_distro_idx(sel_idx as i32);
                    if let Some(d) = app.get_url_distro_list().row_data(sel_idx) {
                        let suffix = if is_arm64 { "-arm64" } else { "" };
                        let instance_name = format!("{}{}", d.name, suffix);
                        app.set_selected_install_distro(d.friendly_name.clone());
                        app.set_new_instance_name(instance_name.clone().into());
                        let base = app.get_distro_location().to_string();
                        app.set_new_instance_path((base + "\\" + &instance_name).into());
                    }
                } else {
                    app.set_selected_url_distro_idx(-1);
                }
            }
        });
    }

    // Fetch URL distribution list
    let ah_fetch = app_handle.clone();
    let cache_fetch = url_entries_cache.clone();
    app.on_fetch_url_distros(move || {
        let ah = ah_fetch.clone();
        let cache = cache_fetch.clone();
        let _ = slint::spawn_local(async move {
            if let Some(app) = ah.upgrade() {
                app.set_is_fetching_url_distros(true);
                app.set_task_status_text(i18n::t("add.url.fetching").into());
                app.set_task_status_visible(true);
            }

            // Determine the URL to fetch
            let url = {
                if let Some(app) = ah.upgrade() {
                    let idx = app.get_selected_install_url_idx();
                    if idx == 2 {
                        app.get_custom_install_url().to_string()
                    } else {
                        crate::download::get_default_url(idx as usize).to_string()
                    }
                } else {
                    return;
                }
            };

            if url.is_empty() {
                if let Some(app) = ah.upgrade() {
                    app.set_is_fetching_url_distros(false);
                    app.set_task_status_visible(false);
                    app.set_current_message(i18n::t("add.url.custom_placeholder").into());
                    app.set_show_message_dialog(true);
                }
                return;
            }

            // Fetch or read the JSON
            let result = if is_local_path(&url) {
                let local_path = strip_file_protocol(&url);
                let path_clone = local_path.clone();
                tokio::task::spawn_blocking(move || {
                    std::fs::read_to_string(&path_clone)
                        .map_err(|e| format!("Failed to read local file '{}': {}", local_path, e))
                })
                .await
                .unwrap_or(Err("Async task failed".to_string()))
            } else {
                crate::download::fetch_distribution_json(&url).await
            };

            match result {
                Ok(json) => {
                    match crate::download::parse_distribution_info(&json) {
                        Ok((entries, _default_idx)) => {
                            // Cache all entries
                            *cache.lock().await = Some(entries);
                            // Rebuild models for current architecture
                            rebuild_url_models_from(&ah, &cache);
                        }
                        Err(e) => {
                            if let Some(app) = ah.upgrade() {
                                app.set_current_message(
                                    i18n::tr("add.url.fetch_failed", &[e]).into(),
                                );
                                app.set_show_message_dialog(true);
                            }
                        }
                    }
                }
                Err(e) => {
                    if let Some(app) = ah.upgrade() {
                        app.set_current_message(i18n::tr("add.url.fetch_failed", &[e]).into());
                        app.set_show_message_dialog(true);
                    }
                }
            }

            if let Some(app) = ah.upgrade() {
                app.set_is_fetching_url_distros(false);
                app.set_task_status_visible(false);
            }
        });
    });

    // Architecture change: rebuild distro list from cache
    let ah_arch = app_handle.clone();
    let cache_arch = url_entries_cache.clone();
    app.on_url_arch_changed(move || {
        rebuild_url_models_from(&ah_arch, &cache_arch);
    });

    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_source_selected(move |idx| {
        if let Some(app) = ah.upgrade() {
            app.set_name_error("".into());
            app.set_path_error("".into());
            app.set_install_status("".into());
            app.set_terminal_output("".into());
        }

        if idx == 3 {
            // Auto-fetch URL distribution list when entering URL Download mode
            let ah_inner = ah.clone();
            let _ = slint::spawn_local(async move {
                if let Some(app) = ah_inner.upgrade() {
                    let url_idx = app.get_selected_install_url_idx();
                    let should_fetch = if url_idx == 2 {
                        is_local_path(&app.get_custom_install_url().to_string())
                    } else {
                        true
                    };
                    if should_fetch {
                        app.invoke_fetch_url_distros();
                    }
                }
            });
        } else if idx == 2 {
            let ah_inner = ah.clone();
            let as_ptr = as_ptr.clone();
            let _ = slint::spawn_local(async move {
                if let Some(app) = ah_inner.upgrade() {
                    if app.get_installable_distro_names().row_count() == 0 {
                        app.set_task_status_text(i18n::t("operation.fetching_distros").into());
                        app.set_task_status_visible(true);

                        refresh_installable_distros(ah_inner.clone(), as_ptr).await;

                        if let Some(app) = ah_inner.upgrade() {
                            app.set_task_status_visible(false);
                        }
                    } else {
                        if let Some(first) = app.get_installable_distros().row_data(0) {
                            let first_id: slint::SharedString = first.name;
                            let first_friendly: slint::SharedString = first.friendly_name;
                            app.set_selected_install_distro(first_id);
                            app.invoke_distro_selected(first_friendly);
                        }
                    }
                }
            });
        }
    });

    let ah = app_handle.clone();
    let as_ptr = app_state.clone();
    app.on_install_distro(
        move |source_idx, name, friendly_name, install_path, file_path| {
            let name = name.to_string();
            let friendly_name = friendly_name.to_string();
            let install_path = install_path.to_string();
            let file_path = file_path.to_string();
            let ah = ah.clone();
            let as_ptr = as_ptr.clone();

            println!(
                "\n[UI Event] on_install_distro: name={}, source={}",
                name, source_idx
            );

            let ah_weak = ah.clone();
            let as_ptr = as_ptr.clone();

            println!(
                "\n[UI Event] on_install_distro: name={}, source={}",
                name, source_idx
            );

            let _ = slint::spawn_local(async move {
                let (manager, internal_id) = if let Some(app) = ah_weak.upgrade() {
                    if app.get_is_installing() {
                        println!("[UI Event] Installation already in progress, ignoring click.");
                        return;
                    }

                    let state = as_ptr.lock().await;
                    (
                        state.wsl_dashboard.clone(),
                        app.get_selected_install_distro().to_string(),
                    )
                } else {
                    return;
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
                    let ah_logic = app.as_weak();
                    let set_pw = app.get_set_root_password();
                    let root_pw = app.get_root_password().to_string();
                    let add_user = app.get_add_new_user();
                    let new_user = app.get_new_username().to_string();
                    let new_pw = app.get_new_user_password().to_string();
                    let set_default = app.get_set_default_user();

                    // URL mode specific parameters
                    let url_download_threads = if source_idx == 3 {
                        app.get_download_threads() as u8
                    } else {
                        4
                    };
                    let _url_is_arm64 = if source_idx == 3 {
                        app.get_url_distro_is_arm64()
                    } else {
                        false
                    };
                    let url_source_idx = if source_idx == 3 {
                        app.get_selected_install_url_idx() as u8
                    } else {
                        0
                    };
                    let custom_url = if source_idx == 3 {
                        app.get_custom_install_url().to_string()
                    } else {
                        String::new()
                    };
                    let url_distro_url = if source_idx == 3 {
                        let list = app.get_url_distro_list();
                        let idx = app.get_selected_url_distro_idx();
                        if idx >= 0 && (idx as usize) < list.row_count() {
                            if let Some(d) = list.row_data(idx as usize) {
                                d.url.to_string()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    };
                    let url_distro_sha256 = if source_idx == 3 {
                        let list = app.get_url_distro_list();
                        let idx = app.get_selected_url_distro_idx();
                        if idx >= 0 && (idx as usize) < list.row_count() {
                            if let Some(d) = list.row_data(idx as usize) {
                                d.sha256.to_string()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    };

                    let _ = tokio::spawn(async move {
                        super::install_logic::perform_install(
                            ah_logic,
                            as_ptr,
                            source_idx,
                            name,
                            friendly_name,
                            internal_id,
                            install_path,
                            file_path,
                            set_pw,
                            root_pw,
                            add_user,
                            new_user,
                            new_pw,
                            set_default,
                            url_download_threads,
                            _url_is_arm64,
                            url_source_idx,
                            custom_url,
                            url_distro_url,
                            url_distro_sha256,
                        )
                        .await;
                    });
                }
            });
        },
    );

    // Check if new username is purely numeric, hide default user if so
    let ah_pw2 = app_handle.clone();
    app.on_check_new_username(move |val: slint::SharedString| {
        let is_numeric = !val.is_empty() && val.chars().all(|c| c.is_ascii_digit());
        let is_empty = val.is_empty();
        let ah = ah_pw2.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = ah.upgrade() {
                app.set_new_username_is_numeric(is_numeric);
                if is_numeric || is_empty {
                    app.set_set_default_user(false);
                }
            }
        });
    });
}
