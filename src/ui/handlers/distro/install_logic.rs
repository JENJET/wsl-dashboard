use super::{generate_random_suffix, sanitize_instance_name};
use crate::ui::data::refresh_distros_ui;
use crate::{AppState, AppWindow, i18n};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

pub async fn perform_install(
    ah: slint::Weak<AppWindow>,
    as_ptr: Arc<Mutex<AppState>>,
    source_idx: i32,
    name: String,
    friendly_name: String,
    internal_id: String,
    install_path: String,
    file_path: String,
    set_root_password: bool,
    root_password: String,
    add_new_user: bool,
    new_username: String,
    new_user_password: String,
    set_default_user: bool,
    // URL mode parameters
    url_threads: u8,
    _url_is_arm64: bool,
    _url_source_idx: u8,
    _custom_url: String,
    url_distro_url: String,
    url_distro_sha256: String,
) {
    // Helper to trigger scroll-to-bottom for page
    fn trigger_scroll(ah: &slint::Weak<AppWindow>) {
        let ah = ah.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = ah.upgrade() {
                app.set_page_vp_y(-99999.0);
            }
        });
    }

    info!(
        "perform_install started: source={}, name={}, friendly={}, internal_id={}, path={}",
        source_idx, name, friendly_name, internal_id, install_path
    );

    // Guard against UI thread blocks - yield initially
    tokio::task::yield_now().await;

    // 2. Setup initial state and manual operation guard
    let (dashboard, executor, config_manager, distro_snapshot) = {
        let lock_timeout = std::time::Duration::from_millis(3000);
        match tokio::time::timeout(lock_timeout, as_ptr.lock()).await {
            Ok(state) => {
                // Get a snapshot of distros for conflict check (using async to avoid deadlock)
                let distros = state.wsl_dashboard.get_distros().await;
                (
                    Arc::new(state.wsl_dashboard.clone()),
                    state.wsl_dashboard.executor().clone(),
                    state.config_manager.clone(),
                    distros,
                )
            }
            Err(_) => {
                error!("perform_install: Failed to acquire AppState lock within 3s");
                let ah_err = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_err.upgrade() {
                        let app_typed: AppWindow = app;
                        app_typed.set_install_status(i18n::t("install.error").into());
                        app_typed.set_terminal_output(
                            "Error: System is busy (AppState lock timeout). Please try again."
                                .into(),
                        );
                    }
                });
                return;
            }
        }
    };

    dashboard.increment_manual_operation();
    let dashboard_cleanup = dashboard.clone();
    let _manual_op_guard = scopeguard::guard(dashboard_cleanup, |db| {
        db.decrement_manual_operation();
    });

    // 2.5 Acquire heavy operation lock
    let _heavy_lock = match dashboard.heavy_op_lock().try_lock() {
        Ok(lock) => lock,
        Err(_) => {
            error!("perform_install: Heavy operation lock is already held");
            let ah_err = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_err.upgrade() {
                    let app_typed: AppWindow = app;
                    app_typed.set_is_installing(false);
                    app_typed.set_install_status(i18n::t("toast.system_busy").into());
                }
            });
            return;
        }
    };

    info!("perform_install: Initializing UI state...");
    let ah_init = ah.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = ah_init.upgrade() {
            let app_typed: AppWindow = app;
            app_typed.set_is_installing(true);
            app_typed.set_install_status(i18n::t("install.checking").into());
            app_typed.set_install_success(false);
            app_typed.set_terminal_output("".into());
            app_typed.set_name_error("".into());
        }
    });

    // 3. Name validation and conflict detection
    let mut final_name = name.clone();
    if final_name.is_empty() {
        if source_idx == 2 {
            final_name = friendly_name.clone();
        } else if !file_path.is_empty() {
            if let Some(stem) = std::path::Path::new(&file_path).file_stem() {
                final_name = stem.to_string_lossy().to_string();
            }
        }
    }

    if final_name.is_empty() {
        let ah_err = ah.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = ah_err.upgrade() {
                let app_typed: AppWindow = app;
                app_typed.set_name_error(i18n::t("dialog.name_required").into());
                app_typed.set_is_installing(false);
                app_typed.set_install_status(i18n::t("install.error").into());
            }
        });
        return;
    }

    let is_valid_chars = final_name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    if !is_valid_chars || final_name.len() > 25 {
        let ah_err = ah.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = ah_err.upgrade() {
                let app_typed: AppWindow = app;
                app_typed.set_name_error(i18n::t("dialog.install_name_invalid").into());
                app_typed.set_is_installing(false);
                app_typed.set_install_status(i18n::t("install.error").into());
            }
        });
        return;
    }

    let name_exists = distro_snapshot.iter().any(|d| d.name == final_name);

    if name_exists {
        let new_suggested_name = sanitize_instance_name(&generate_random_suffix(&final_name));
        let distro_location = config_manager.get_settings().distro_location.clone();
        let ah_err = ah.clone();

        let new_path = std::path::Path::new(&distro_location)
            .join(&new_suggested_name)
            .to_string_lossy()
            .to_string();

        let final_name_clone = final_name.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = ah_err.upgrade() {
                let app_typed: AppWindow = app;
                app_typed.set_new_instance_name(new_suggested_name.into());
                app_typed.set_new_instance_path(new_path.into());
                app_typed.set_name_error(
                    i18n::tr("dialog.install_name_exists", &[final_name_clone]).into(),
                );
                app_typed.set_is_installing(false);
                app_typed.set_install_status(i18n::t("install.conflict_error").into());
            }
        });
        return;
    }

    let mut success = false;
    let mut error_msg = String::new();

    // 4. Source-specific installation logic
    match source_idx {
        2 => {
            // Store Source
            let real_id = if !internal_id.is_empty() {
                internal_id.clone()
            } else {
                // Fallback for custom RootFS/VHDX if applicable, though usually they go through different match arms
                friendly_name.clone()
            };

            if real_id.is_empty() {
                let ah_err = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_err.upgrade() {
                        let app_typed: AppWindow = app;
                        app_typed.set_install_status(i18n::t("install.unknown_distro").into());
                        app_typed.set_is_installing(false);
                    }
                });
                return;
            }

            let ah_status = ah.clone();
            let real_id_clone = real_id.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_status.upgrade() {
                    let app_typed: AppWindow = app;
                    app_typed.set_install_status(i18n::t("install.installing").into());
                    app_typed.set_terminal_output(
                        format!("{}\n", i18n::tr("install.step_1", &[real_id_clone])).into(),
                    );
                }
            });
            trigger_scroll(&ah);
            let mut terminal_buffer =
                format!("{}\n", i18n::tr("install.step_1", &[real_id.clone()]));
            info!(
                "Starting store installation for distribution ID: {}",
                real_id
            );

            // Check if real_id already exists in WSL to prevent accidental data loss
            if distro_snapshot.iter().any(|d| d.name == real_id) {
                let ah_err = ah.clone();
                let real_id_err = real_id.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_err.upgrade() {
                        let app_typed: AppWindow = app;
                        app_typed.set_name_error(
                            i18n::tr("install.real_id_conflict", &[real_id_err]).into(),
                        );
                        app_typed.set_is_installing(false);
                        app_typed.set_install_status(i18n::t("install.conflict_error").into());
                    }
                });
                return;
            }

            // Cleanup existing if any
            let _ = executor.delete_distro(&config_manager, &real_id).await;

            terminal_buffer.push_str(&format!("{}\n", i18n::t("install.step_2")));
            let ah_cb = ah.clone();
            let tb_clone = terminal_buffer.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_cb.upgrade() {
                    let app_typed: AppWindow = app;
                    app_typed.set_terminal_output(tb_clone.into());
                }
            });

            // Detect fastest source can involve network calls
            let use_web_download = executor.detect_fastest_source().await;

            let mut install_args = vec!["--install", "-d", &real_id, "--no-launch"];
            if use_web_download {
                install_args.push("--web-download");
            }
            let cmd_str = format!("wsl {}", install_args.join(" "));

            terminal_buffer.push_str(&format!(
                "{}\n",
                i18n::tr("install.step_3", &[cmd_str.clone()])
            ));
            let source_text = if use_web_download {
                "GitHub"
            } else {
                "Microsoft"
            };
            terminal_buffer.push_str(&i18n::tr("install.step_4", &[source_text.to_string()]));

            let ah_cb = ah.clone();
            let tb_clone = terminal_buffer.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_cb.upgrade() {
                    let app_typed: AppWindow = app;
                    app_typed.set_terminal_output(tb_clone.into());
                }
            });

            info!("Installing from source: {}", source_text);

            let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(100);

            // Channel-based UI update task to throttle updates and prevent freezing
            let ah_ui = ah.clone();
            let initial_tb = terminal_buffer.clone();
            let ui_task = tokio::spawn(async move {
                let mut buffer = initial_tb;
                let mut dot_count = 0;
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(800));

                loop {
                    tokio::select! {
                        msg = rx.recv() => {
                            if msg.is_none() {
                                break; // Channel closed
                            }
                            // We consume the messages but don't append to buffer to hide all WSL output
                        }
                        _ = interval.tick() => {
                            // Only add dots if the current line is an "active" one (doesn't end in newline)
                            if !buffer.ends_with('\n') {
                                dot_count = (dot_count % 3) + 1; // Always show 1, 2, or 3 dots
                                let mut dots = String::new();
                                for _ in 0..dot_count { dots.push('.'); }
                                let text_to_set = format!("{}{}", buffer, dots);

                                let ah_cb = ah_ui.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(app) = ah_cb.upgrade() {
                                        app.set_terminal_output(text_to_set.into());
                                    }
                                });
                            }
                        }
                    }

                    if buffer.len() > 20_000 {
                        let to_drain = buffer.len() - 10_000;
                        if let Some(pos) = buffer[to_drain..].find('\n') {
                            buffer.drain(..to_drain + pos + 1);
                        } else {
                            buffer.drain(..to_drain);
                        }
                    }

                    // Throttled UI update removed to prevent overwriting dots animation
                    // since all WSL output is hidden in this phase.
                }

                // No final flush needed since we are hiding all WSL output

                let ah_final = ah_ui.clone();
                let text_to_set = buffer.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_final.upgrade() {
                        let app_typed: AppWindow = app;
                        app_typed.set_terminal_output(text_to_set.into());
                    }
                });
                buffer
            });

            info!("Waiting for WSL installation to complete...");
            let tx_callback = tx.clone();
            let result = executor
                .execute_command_streaming(&install_args, move |text| {
                    let _ = tx_callback.try_send(text);
                })
                .await;

            drop(tx);
            terminal_buffer = ui_task.await.unwrap_or(terminal_buffer);
            // Don't add newline yet, verification will keep dots rolling
            if !terminal_buffer.ends_with('.') && !terminal_buffer.ends_with('\n') {
                terminal_buffer.push_str("."); // Start with one dot to bridge the gap
            }

            let ah_res = ah.clone();
            let tb_clone = terminal_buffer.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_res.upgrade() {
                    let app_typed: AppWindow = app;
                    app_typed.set_terminal_output(tb_clone.into());
                }
            });

            if result.success {
                let mut distro_registered = false;
                let ah_status = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_status.upgrade() {
                        let app_typed: AppWindow = app;
                        app_typed.set_install_status(i18n::t("install.verifying").into());
                    }
                });

                let mut verify_dot_count = 1;
                for _ in 0..15 {
                    dashboard.refresh_distros().await;

                    let distros_final = dashboard.get_distros().await;
                    if distros_final.iter().any(|d| d.name == real_id) {
                        distro_registered = true;
                        break;
                    }

                    // Keep dots rolling even during verification (2s sleep -> small steps)
                    for _ in 0..3 {
                        tokio::time::sleep(std::time::Duration::from_millis(666)).await;
                        verify_dot_count = (verify_dot_count % 3) + 1;
                        let mut dots = String::new();
                        for _ in 0..verify_dot_count {
                            dots.push('.');
                        }
                        let text_to_set = format!("{}{}", terminal_buffer, dots);

                        let ah_v = ah.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah_v.upgrade() {
                                app.set_terminal_output(text_to_set.into());
                            }
                        });
                    }
                }

                // Final dots and newline before next step
                terminal_buffer.push_str("...");
                terminal_buffer.push('\n');

                if !distro_registered {
                    error_msg = i18n::tr("install.verify_failed", &[real_id.clone()]);
                } else {
                    terminal_buffer.push_str(&format!("{}\n", i18n::t("install.step_5")));
                    let ah_cb = ah.clone();
                    let tb_clone = terminal_buffer.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_cb.upgrade() {
                            let app_typed: AppWindow = app;
                            app_typed.set_terminal_output(tb_clone.into());
                        }
                    });

                    if final_name != real_id || !install_path.is_empty() {
                        info!("Relocating distribution to {}...", install_path);
                        let ah_cb = ah.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah_cb.upgrade() {
                                let app_typed: AppWindow = app;
                                app_typed.set_install_status(i18n::t("install.customizing").into());
                            }
                        });

                        terminal_buffer.push_str(&format!("{}\n", i18n::t("install.step_6")));
                        let ah_cb = ah.clone();
                        let tb_clone = terminal_buffer.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah_cb.upgrade() {
                                let app_typed: AppWindow = app;
                                app_typed.set_terminal_output(tb_clone.into());
                            }
                        });

                        let (temp_dir, temp_file_str) = {
                            let temp_location = config_manager.get_settings().temp_location.clone();
                            let temp_dir = PathBuf::from(temp_location);
                            let temp_file =
                                temp_dir.join(format!("wsl_move_{}.tar", uuid::Uuid::new_v4()));
                            (temp_dir, temp_file.to_string_lossy().to_string())
                        };

                        let _ =
                            tokio::task::spawn_blocking(move || std::fs::create_dir_all(&temp_dir))
                                .await;
                        let target_path = install_path.clone();

                        tokio::task::yield_now().await;
                        executor
                            .execute_command(&["--export", &real_id, &temp_file_str])
                            .await;

                        tokio::task::yield_now().await;
                        terminal_buffer.push_str(&format!("{}\n", i18n::t("install.step_7")));
                        let ah_cb = ah.clone();
                        let tb_clone = terminal_buffer.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah_cb.upgrade() {
                                let app_typed: AppWindow = app;
                                app_typed.set_terminal_output(tb_clone.into());
                            }
                        });

                        tokio::task::yield_now().await;
                        executor.execute_command(&["--unregister", &real_id]).await;

                        let final_path = if target_path.is_empty() {
                            let distro_location =
                                config_manager.get_settings().distro_location.clone();
                            let base = PathBuf::from(&distro_location);
                            base.join(&final_name).to_string_lossy().to_string()
                        } else {
                            target_path
                        };

                        let fp_clone = final_path.clone();
                        let _ =
                            tokio::task::spawn_blocking(move || std::fs::create_dir_all(&fp_clone))
                                .await;

                        tokio::task::yield_now().await;
                        let import_res = executor
                            .execute_command(&[
                                "--import",
                                &final_name,
                                &final_path,
                                &temp_file_str,
                            ])
                            .await;

                        let tf_clone = temp_file_str.clone();
                        let _ =
                            tokio::task::spawn_blocking(move || std::fs::remove_file(&tf_clone))
                                .await;

                        success = import_res.success;
                        if success {
                            terminal_buffer.push_str(&format!("{}\n", i18n::t("install.step_8")));
                            terminal_buffer.push_str(&format!("{}\n", i18n::t("install.step_9")));
                        } else {
                            error_msg = import_res
                                .error
                                .unwrap_or_else(|| i18n::t("install.import_failed_custom"));
                        }
                    } else {
                        success = true;
                        terminal_buffer.push_str(&format!("{}\n", i18n::t("install.step_9")));
                    }

                    let ah_cb = ah.clone();
                    let tb_clone = terminal_buffer.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_cb.upgrade() {
                            let app_typed: AppWindow = app;
                            app_typed.set_terminal_output(tb_clone.into());
                        }
                    });
                }
            } else {
                if !result.output.trim().is_empty() {
                    terminal_buffer.push_str(&format!("\n[WSL Output]\n{}\n", result.output));
                }
                error_msg = result
                    .error
                    .unwrap_or_else(|| i18n::t("install.install_failed"));
                let ah_cb = ah.clone();
                let tb_clone = terminal_buffer.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_cb.upgrade() {
                        let app_typed: AppWindow = app;
                        app_typed.set_terminal_output(tb_clone.into());
                    }
                });
            }
        }
        3 => {
            // URL Download + Import
            let download_url = if !url_distro_url.is_empty() {
                url_distro_url.clone()
            } else {
                error_msg = i18n::t("install.url.step_no_url");
                let ah_err = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_err.upgrade() {
                        let app_typed: AppWindow = app;
                        app_typed.set_install_status(
                            format!("{}: {}", i18n::t("install.error"), error_msg).into(),
                        );
                        app_typed.set_is_installing(false);
                    }
                });
                return;
            };

            let mut terminal_buffer = String::new();

            // Determine cache directory
            let cache_dir = {
                let lock_timeout = std::time::Duration::from_millis(3000);
                match tokio::time::timeout(lock_timeout, as_ptr.lock()).await {
                    Ok(state) => {
                        let distro_loc =
                            state.config_manager.get_settings().distro_location.clone();
                        std::path::PathBuf::from(distro_loc).join("download")
                    }
                    Err(_) => {
                        let ah_err = ah.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah_err.upgrade() {
                                app.set_install_status(i18n::t("install.error").into());
                                app.set_is_installing(false);
                            }
                        });
                        return;
                    }
                }
            };

            let threads = url_threads.max(1).min(8) as usize;

            // [1/4] Download
            let step1 = i18n::tr("install.url.step_1_4", &[download_url.clone()]);
            let thread_info = i18n::tr("install.url.step_threads", &[threads.to_string()]);
            terminal_buffer.push_str(&format!("{}\n", step1));
            terminal_buffer.push_str(&format!("    {}\n", thread_info));

            let ah_cb = ah.clone();
            let tb_clone = terminal_buffer.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_cb.upgrade() {
                    let app_typed: AppWindow = app;
                    app_typed.set_install_status(step1.clone().into());
                    app_typed.set_terminal_output(tb_clone.into());
                }
            });
            trigger_scroll(&ah);

            let download_manager = crate::download::DownloadManager::new(cache_dir);
            let dl_url = download_url.clone();
            let dl_sha256 = url_distro_sha256.clone();

            // Shared state: dl_progress for terminal line, dl_status for install_status
            let dl_progress = Arc::new(std::sync::Mutex::new(String::new()));
            let dl_status = Arc::new(std::sync::Mutex::new(String::new()));
            let dl_progress_clone = dl_progress.clone();
            let dl_status_clone = dl_status.clone();
            let ui_dl_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let ui_dl_stop_clone = ui_dl_stop.clone();

            let ah_ui_dl = ah.clone();
            let tb_dl = terminal_buffer.clone();
            let ui_dl_task = tokio::spawn(async move {
                let mut buffer = tb_dl;
                let mut dot_count = 0;
                let mut last_status = String::new();
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    if ui_dl_stop_clone.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    dot_count = (dot_count % 3) + 1;
                    let dots = ".".repeat(dot_count);

                    // Check for new status (retry) message
                    let status = dl_status_clone.lock().unwrap().clone();
                    if !status.is_empty() && status != last_status {
                        // Append new status line to buffer permanently
                        buffer.push_str(&format!("    {}\n", status));
                        last_status = status.clone();
                    }

                    // Show download progress in the active line
                    let progress = dl_progress_clone.lock().unwrap().clone();
                    let line = if !progress.is_empty() {
                        format!("    {} {}", progress, dots)
                    } else {
                        format!("    {}", dots)
                    };
                    let text = format!("{}{}", buffer, line);
                    let progress_clone = progress.clone();
                    let ah_cb = ah_ui_dl.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_cb.upgrade() {
                            app.set_terminal_output(text.into());
                            app.set_task_status_text(progress_clone.into());
                        }
                    });
                }
                buffer
            });

            let dl_progress_cb = dl_progress.clone();
            let dl_status_cb = dl_status.clone();
            let download_result = download_manager
                .download(
                    &final_name,
                    &dl_url,
                    &dl_sha256,
                    threads,
                    Some(move |current: u64, total: u64| {
                        let current_mb = current as f64 / (1024.0 * 1024.0);
                        let total_mb = total as f64 / (1024.0 * 1024.0);
                        let pct = if total > 0 {
                            current as f64 / total as f64 * 100.0
                        } else {
                            0.0
                        };
                        let status = i18n::tr(
                            "install.url.step_1_4_progress",
                            &[
                                format!("{:.2}", current_mb),
                                format!("{:.2}", total_mb),
                                format!("{:.2}", pct),
                            ],
                        );
                        if let Ok(mut p) = dl_progress_cb.lock() {
                            *p = status;
                        }
                    }),
                    Some(move |msg: String| {
                        if msg.starts_with("verify_failed/") {
                            let retry_count = msg.trim_start_matches("verify_failed/").to_string();
                            let fail_msg = i18n::tr(
                                "install.url.step_2_4_failed",
                                &[retry_count, 3.to_string()],
                            );
                            if let Ok(mut s) = dl_status_cb.lock() {
                                *s = fail_msg;
                            }
                        }
                    }),
                )
                .await;

            ui_dl_stop.store(true, std::sync::atomic::Ordering::Relaxed);
            terminal_buffer = ui_dl_task.await.unwrap_or(terminal_buffer);

            match download_result {
                Ok(cached_path) => {
                    // [2/4] Verify done
                    let step2 = i18n::t("install.url.step_2_4_done");
                    terminal_buffer.push_str(&format!("{}\n", step2));
                    let ah_cb = ah.clone();
                    let tb_clone = terminal_buffer.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_cb.upgrade() {
                            app.set_install_status(step2.clone().into());
                            app.set_terminal_output(tb_clone.into());
                        }
                    });

                    // [3/4] Import
                    let step3 = i18n::t("install.url.step_3_4");
                    terminal_buffer.push_str(&format!("{}\n", step3));
                    let ah_cb = ah.clone();
                    let tb_clone = terminal_buffer.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_cb.upgrade() {
                            app.set_install_status(step3.clone().into());
                            app.set_terminal_output(tb_clone.into());
                        }
                    });

                    let mut target_path = install_path.clone();
                    if target_path.is_empty() {
                        let distro_location = config_manager.get_settings().distro_location.clone();
                        let base = std::path::PathBuf::from(&distro_location);
                        target_path = base.join(&final_name).to_string_lossy().to_string();
                    }

                    let tp_clone = target_path.clone();
                    if let Err(e) =
                        tokio::task::spawn_blocking(move || std::fs::create_dir_all(&tp_clone))
                            .await
                            .unwrap()
                    {
                        error_msg = format!("Failed to create directory: {}", e);
                        let ah_err = ah.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah_err.upgrade() {
                                app.set_install_success(false);
                                app.set_install_status(
                                    format!("{}: {}", i18n::t("install.error"), error_msg).into(),
                                );
                                app.set_is_installing(false);
                            }
                        });
                        return;
                    }

                    let cached_path_str = cached_path.to_string_lossy().to_string();
                    let import_args = vec!["--import", &final_name, &target_path, &cached_path_str];

                    let cmd_str = format!("wsl {}", import_args.join(" "));
                    terminal_buffer.push_str(&format!("    {}\n", cmd_str));

                    let ah_cb = ah.clone();
                    let tb_clone = terminal_buffer.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_cb.upgrade() {
                            app.set_terminal_output(tb_clone.into());
                        }
                    });

                    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(100);
                    let ah_ui = ah.clone();
                    let initial_tb = terminal_buffer.clone();
                    let ui_task = tokio::spawn(async move {
                        let mut buffer = initial_tb;
                        let mut dot_count = 0;
                        let mut interval =
                            tokio::time::interval(std::time::Duration::from_millis(800));
                        loop {
                            tokio::select! {
                                msg = rx.recv() => {
                                    if msg.is_none() { break; }
                                }
                                _ = interval.tick() => {
                                    if !buffer.ends_with('\n') {
                                        dot_count = (dot_count % 3) + 1;
                                        let dots = ".".repeat(dot_count);
                                        let text = format!("{}{}", buffer, dots);
                                        let ah_cb = ah_ui.clone();
                                        let _ = slint::invoke_from_event_loop(move || {
                                            if let Some(app) = ah_cb.upgrade() {
                                                app.set_terminal_output(text.into());
                                            }
                                        });
                                    }
                                }
                            }
                            if buffer.len() > 20_000 {
                                let to_drain = buffer.len() - 10_000;
                                if let Some(pos) = buffer[to_drain..].find('\n') {
                                    buffer.drain(..to_drain + pos + 1);
                                } else {
                                    buffer.drain(..to_drain);
                                }
                            }
                        }
                        if !buffer.ends_with('\n') {
                            buffer.push('\n');
                        }
                        let ah_final = ah_ui.clone();
                        let text = buffer.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah_final.upgrade() {
                                app.set_terminal_output(text.into());
                            }
                        });
                        buffer
                    });

                    let tx_callback = tx.clone();
                    let result = executor
                        .execute_command_streaming(&import_args, move |text| {
                            let _ = tx_callback.try_send(text);
                        })
                        .await;

                    drop(tx);
                    terminal_buffer = ui_task.await.unwrap_or(terminal_buffer);

                    success = result.success;
                    if !success {
                        if !result.output.trim().is_empty() {
                            terminal_buffer
                                .push_str(&format!("\n[WSL Output]\n{}\n", result.output));
                        }
                        error_msg = result
                            .error
                            .unwrap_or_else(|| i18n::t("install.import_failed"));
                    } else {
                        // [4/4] Done
                        terminal_buffer.push_str(&format!(
                            "{}\n",
                            i18n::tr("install.url.step_4_4", &[final_name.clone()])
                        ));
                    }

                    let ah_cb = ah.clone();
                    let tb_clone = terminal_buffer.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_cb.upgrade() {
                            app.set_terminal_output(tb_clone.into());
                        }
                    });
                }
                Err(e) => {
                    error_msg = i18n::tr("install.url.step_download_failed", &[3.to_string(), e]);
                    let ah_err = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_err.upgrade() {
                            app.set_install_status(
                                format!("{}: {}", i18n::t("install.error"), error_msg).into(),
                            );
                            app.set_is_installing(false);
                        }
                    });
                    return;
                }
            }
        }
        0 | 1 => {
            // RootFS or VHDX Import
            if file_path.is_empty() {
                error_msg = i18n::t("install.select_file");
            } else {
                let mut terminal_buffer = format!("{}\n", i18n::t("install.step_1_3"));
                let ah_cb = ah.clone();
                let tb_clone = terminal_buffer.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_cb.upgrade() {
                        let app_typed: AppWindow = app;
                        app_typed.set_terminal_output(tb_clone.into());
                    }
                });
                trigger_scroll(&ah);

                let mut target_path = install_path.clone();
                if target_path.is_empty() {
                    let distro_location = config_manager.get_settings().distro_location.clone();
                    let base = PathBuf::from(&distro_location);
                    target_path = base.join(&final_name).to_string_lossy().to_string();
                }

                let tp_clone = target_path.clone();
                if let Err(e) =
                    tokio::task::spawn_blocking(move || std::fs::create_dir_all(&tp_clone))
                        .await
                        .unwrap()
                {
                    let err = format!("Failed to create directory: {}", e);
                    let ah_cb = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_cb.upgrade() {
                            let app_typed: AppWindow = app;
                            app_typed.set_install_success(false);
                            app_typed.set_install_status(
                                format!("{}: {}", i18n::t("install.error"), err).into(),
                            );
                            app_typed.set_is_installing(false);
                        }
                    });
                    return;
                }

                let ah_cb = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_cb.upgrade() {
                        let app_typed: AppWindow = app;
                        app_typed.set_install_status(i18n::t("install.importing").into());
                    }
                });

                let mut import_args = vec!["--import", &final_name, &target_path, &file_path];
                if source_idx == 1 {
                    import_args.push("--vhd");
                }

                let cmd_str = format!("wsl {}", import_args.join(" "));
                terminal_buffer.push_str(&i18n::tr("install.step_2_3", &[cmd_str.clone()]));
                let ah_cb = ah.clone();
                let tb_clone = terminal_buffer.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_cb.upgrade() {
                        let app_typed: AppWindow = app;
                        app_typed.set_terminal_output(tb_clone.into());
                    }
                });

                let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(100);
                let ah_ui = ah.clone();
                let initial_tb = terminal_buffer.clone();
                let ui_task = tokio::spawn(async move {
                    let mut buffer = initial_tb;
                    let mut dot_count = 0;
                    let mut interval = tokio::time::interval(std::time::Duration::from_millis(800));

                    loop {
                        tokio::select! {
                            msg = rx.recv() => {
                                if msg.is_none() {
                                    break;
                                }
                                // Consume but don't display
                            }
                            _ = interval.tick() => {
                                if !buffer.ends_with('\n') {
                                     dot_count = (dot_count % 3) + 1;
                                     let mut dots = String::new();
                                     for _ in 0..dot_count { dots.push('.'); }
                                     let text_to_set = format!("{}{}", buffer, dots);

                                     let ah_cb = ah_ui.clone();
                                     let _ = slint::invoke_from_event_loop(move || {
                                         if let Some(app) = ah_cb.upgrade() {
                                             app.set_terminal_output(text_to_set.into());
                                         }
                                     });
                                }
                            }
                        }

                        if buffer.len() > 20_000 {
                            let to_drain = buffer.len() - 10_000;
                            if let Some(pos) = buffer[to_drain..].find('\n') {
                                buffer.drain(..to_drain + pos + 1);
                            } else {
                                buffer.drain(..to_drain);
                            }
                        }
                        // Throttled UI update removed to prevent overwriting dots animation
                    }
                    if !buffer.ends_with('\n') {
                        buffer.push('\n');
                    }
                    let ah_final = ah_ui.clone();
                    let text_to_set = buffer.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_final.upgrade() {
                            let app_typed: AppWindow = app;
                            app_typed.set_terminal_output(text_to_set.into());
                        }
                    });
                    buffer
                });

                let tx_callback = tx.clone();
                let result = executor
                    .execute_command_streaming(&import_args, move |text| {
                        let _ = tx_callback.try_send(text);
                    })
                    .await;

                drop(tx);
                terminal_buffer = ui_task.await.unwrap_or(terminal_buffer);

                success = result.success;
                if !success {
                    if !result.output.trim().is_empty() {
                        terminal_buffer.push_str(&format!("\n[WSL Output]\n{}\n", result.output));
                    }
                    error_msg = result
                        .error
                        .unwrap_or_else(|| i18n::t("install.import_failed"));
                } else {
                    terminal_buffer.push_str(&format!(
                        "{}\n",
                        i18n::tr("install.step_3_3", &[final_name.clone()])
                    ));
                }

                let ah_cb = ah.clone();
                let tb_clone = terminal_buffer.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_cb.upgrade() {
                        let app_typed: AppWindow = app;
                        app_typed.set_terminal_output(tb_clone.into());
                    }
                });
            }
        }
        _ => {
            error_msg = i18n::t("install.unknown_source");
        }
    }

    // Set root password if requested (after successful install)
    if success && set_root_password && !root_password.is_empty() {
        info!("Setting root password for distro '{}'", final_name);

        let step_text = i18n::t("install.step_set_root_password");
        let ah_pw = ah.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = ah_pw.upgrade() {
                let app_typed: AppWindow = app;
                app_typed.set_install_status(step_text.clone().into());
                let mut tb = app_typed.get_terminal_output().to_string();
                tb.push_str(&format!("\n--> {}\n", step_text));
                app_typed.set_terminal_output(tb.into());
            }
        });
        // Start the distro first to ensure it's fully initialized
        info!("Starting distro '{}' before setting password", final_name);
        let _ = executor
            .execute_command(&["-d", &final_name, "-u", "root", "-e", "/bin/true"])
            .await;
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // Escape password for safe use in shell heredoc
        let pw_safe = root_password
            .replace('\\', "\\\\")
            .replace('$', "\\$")
            .replace('`', "\\`");

        // Use heredoc (more reliable than pipe in WSL) with chpasswd at /usr/sbin
        let pw_cmd = format!(
            "/usr/sbin/chpasswd 2>/dev/null <<'PWEOF'\nroot:{}\nPWEOF\n",
            pw_safe
        );
        let set_pw_args = ["-d", &final_name, "-u", "root", "--", "sh", "-c", &pw_cmd];
        info!("Root password command: wsl {}", set_pw_args.join(" "));
        let pw_result = executor.execute_command(&set_pw_args).await;
        info!(
            "Root password result: success={}, output={:?}, error={:?}",
            pw_result.success, pw_result.output, pw_result.error
        );

        // Retry with /sbin/chpasswd if first attempt failed
        let pw_result = if !pw_result.success {
            info!("Retrying with /sbin/chpasswd...");
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let pw_cmd2 = format!("/sbin/chpasswd <<'PWEOF'\nroot:{}\nPWEOF\n", pw_safe);
            let retry_args = ["-d", &final_name, "-u", "root", "--", "sh", "-c", &pw_cmd2];
            let retry_result = executor.execute_command(&retry_args).await;
            info!(
                "Retry result: success={}, output={:?}, error={:?}",
                retry_result.success, retry_result.output, retry_result.error
            );
            retry_result
        } else {
            pw_result
        };

        if pw_result.success {
            info!("Root password set successfully for distro '{}'", final_name);
            let done_text = i18n::t("install.step_set_root_password_done");
            let ah_pw = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_pw.upgrade() {
                    let app_typed: AppWindow = app;
                    app_typed.set_install_status(done_text.clone().into());
                    let mut tb = app_typed.get_terminal_output().to_string();
                    tb.push_str(&format!("      {}\n", done_text));
                    app_typed.set_terminal_output(tb.into());
                }
            });
        } else {
            let pw_output = pw_result.output.clone();
            let pw_err = pw_result.error.unwrap_or_else(|| {
                if !pw_output.trim().is_empty() {
                    pw_output.trim().to_string()
                } else {
                    "Unknown error".to_string()
                }
            });
            error!(
                "Failed to set root password for '{}': {}",
                final_name, pw_err
            );
            let fail_text = i18n::tr("install.step_set_root_password_failed", &[pw_err.clone()]);
            error_msg = fail_text.clone();
            success = false;

            let ah_pw = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_pw.upgrade() {
                    let app_typed: AppWindow = app;
                    app_typed.set_install_status(fail_text.clone().into());
                    let mut tb = app_typed.get_terminal_output().to_string();
                    tb.push_str(&format!("[FAIL] {}\n", fail_text));
                    if !pw_output.trim().is_empty() {
                        tb.push_str(&format!("      {}\n", pw_output.trim()));
                    }
                    app_typed.set_terminal_output(tb.into());
                }
            });
        }
    }

    // Create new user if requested (after root password)
    if success && add_new_user && !new_username.is_empty() {
        info!(
            "Creating new user '{}' for distro '{}'",
            new_username, final_name
        );

        // Step: Create user
        let create_step = i18n::tr("install.step_create_user", &[new_username.clone()]);
        let ah_ui = ah.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = ah_ui.upgrade() {
                let app_typed: AppWindow = app;
                app_typed.set_install_status(create_step.clone().into());
                let mut tb = app_typed.get_terminal_output().to_string();
                tb.push_str(&format!("\n--> {}\n", create_step));
                app_typed.set_terminal_output(tb.into());
            }
        });

        // Always use --badname to allow non-standard usernames, with fallbacks
        let create_cmd = format!(
            "(useradd -m -s /bin/bash {0} 2>&1) || (useradd -m -s /bin/bash --badname {0} 2>&1) || (adduser -D -s /bin/bash {0} 2>&1) || (adduser -s /bin/bash {0} 2>&1)",
            new_username
        );
        let create_args = [
            "-d",
            &final_name,
            "-u",
            "root",
            "--",
            "sh",
            "-c",
            &create_cmd,
        ];
        let create_result = executor.execute_command(&create_args).await;
        info!(
            "useradd result: success={}, output={:?}, error={:?}",
            create_result.success, create_result.output, create_result.error
        );

        // If creation failed, check if user already exists
        let user_exists = if !create_result.success {
            let check_cmd = format!("id {} 2>&1", new_username);
            let check_args = [
                "-d",
                &final_name,
                "-u",
                "root",
                "--",
                "sh",
                "-c",
                &check_cmd,
            ];
            executor.execute_command(&check_args).await.success
        } else {
            false
        };

        let user_ok = create_result.success || user_exists;

        if user_exists {
            info!(
                "User '{}' already exists, treating as success",
                new_username
            );
            let done_text = i18n::tr("install.step_create_user_done", &[new_username.clone()]);
            let ah_ui = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_ui.upgrade() {
                    let app_typed: AppWindow = app;
                    let mut tb = app_typed.get_terminal_output().to_string();
                    tb.push_str(&format!("      {} (already exists)\n", done_text));
                    app_typed.set_terminal_output(tb.into());
                }
            });
        } else if !create_result.success {
            let err = if !create_result
                .error
                .as_ref()
                .map_or(true, |e| e.trim().is_empty())
            {
                create_result.error.unwrap()
            } else if !create_result.output.trim().is_empty() {
                create_result.output.trim().to_string()
            } else {
                "useradd/adduser command not found or failed".to_string()
            };
            error!("Failed to create user '{}': {}", new_username, err);
            let fail_text = i18n::tr(
                "install.step_create_user_failed",
                &[new_username.clone(), err.clone()],
            );
            error_msg = fail_text.clone();
            success = false;
            let ah_ui = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_ui.upgrade() {
                    let app_typed: AppWindow = app;
                    app_typed.set_install_status(fail_text.clone().into());
                    let mut tb = app_typed.get_terminal_output().to_string();
                    tb.push_str(&format!("[FAIL] {}\n    {}\n", fail_text, err));
                    app_typed.set_terminal_output(tb.into());
                }
            });
        } else {
            let done_text = i18n::tr("install.step_create_user_done", &[new_username.clone()]);
            let ah_ui = ah.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = ah_ui.upgrade() {
                    let app_typed: AppWindow = app;
                    let mut tb = app_typed.get_terminal_output().to_string();
                    tb.push_str(&format!("      {}\n", done_text));
                    app_typed.set_terminal_output(tb.into());
                }
            });
        }

        // Add user to sudo/wheel group (if user creation succeeded or already existed)
        if user_ok {
            // Check which group exists (sudo or wheel) and add user to it
            let check_sudo_cmd = "getent group sudo >/dev/null 2>&1 && echo 'sudo' || (getent group wheel >/dev/null 2>&1 && echo 'wheel' || echo '')";
            let check_args = [
                "-d",
                &final_name,
                "-u",
                "root",
                "--",
                "sh",
                "-c",
                check_sudo_cmd,
            ];
            let check_result = executor.execute_command(&check_args).await;
            let group_name = check_result.output.trim();

            if !group_name.is_empty() {
                let sudo_step = i18n::tr(
                    "install.step_add_sudo",
                    &[new_username.clone(), group_name.to_string()],
                );
                let ah_ui = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_ui.upgrade() {
                        let app_typed: AppWindow = app;
                        app_typed.set_install_status(sudo_step.clone().into());
                        let mut tb = app_typed.get_terminal_output().to_string();
                        tb.push_str(&format!("--> {}\n", sudo_step));
                        app_typed.set_terminal_output(tb.into());
                    }
                });

                let add_cmd = format!("usermod -aG {} {} 2>&1", group_name, new_username);
                let add_args = ["-d", &final_name, "-u", "root", "--", "sh", "-c", &add_cmd];
                let add_result = executor.execute_command(&add_args).await;

                if add_result.success {
                    let sudo_done = i18n::tr(
                        "install.step_add_sudo_done",
                        &[new_username.clone(), group_name.to_string()],
                    );
                    info!("User '{}' added to {} group", new_username, group_name);

                    // Ensure /etc/sudoers has the correct group configuration
                    let sudoers_cmd = format!(
                        r#"
                        if ! grep -q '^%{g}' /etc/sudoers 2>/dev/null; then
                            echo '%{g} ALL=(ALL:ALL) ALL' >> /etc/sudoers
                        fi
                        "#,
                        g = group_name
                    );
                    let sudoers_args = [
                        "-d",
                        &final_name,
                        "-u",
                        "root",
                        "--",
                        "sh",
                        "-c",
                        &sudoers_cmd,
                    ];
                    let _sudoers_result = executor.execute_command(&sudoers_args).await;

                    let ah_ui = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_ui.upgrade() {
                            let app_typed: AppWindow = app;
                            let mut tb = app_typed.get_terminal_output().to_string();
                            tb.push_str(&format!("      {}\n", sudo_done));
                            app_typed.set_terminal_output(tb.into());
                        }
                    });
                } else {
                    let err = add_result
                        .error
                        .unwrap_or_else(|| add_result.output.trim().to_string());
                    let err_msg = if err.is_empty() {
                        i18n::t("install.step_add_sudo_error_unknown")
                    } else {
                        i18n::tr(
                            "install.step_add_sudo_error_detail",
                            &[group_name.to_string(), err.clone()],
                        )
                    };
                    info!(
                        "Failed to add user '{}' to {} group: {}",
                        new_username, group_name, err
                    );
                    let ah_ui = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_ui.upgrade() {
                            let app_typed: AppWindow = app;
                            let mut tb = app_typed.get_terminal_output().to_string();
                            tb.push_str(&format!("[WARN] {}\n", err_msg));
                            app_typed.set_terminal_output(tb.into());
                        }
                    });
                }
            } else {
                let skip_msg = i18n::tr("install.step_add_sudo_skipped", &[]);
                info!(
                    "Neither sudo nor wheel group found, skipping sudo setup for user '{}'",
                    new_username
                );
                let ah_ui = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_ui.upgrade() {
                        let app_typed: AppWindow = app;
                        let mut tb = app_typed.get_terminal_output().to_string();
                        tb.push_str(&format!("      {}\n", skip_msg));
                        app_typed.set_terminal_output(tb.into());
                    }
                });
            }
        }

        // Set password and default user (if user creation succeeded or already existed)
        if user_ok {
            // Set password for new user (if provided)
            if !new_user_password.is_empty() {
                let pw_step = i18n::tr("install.step_set_user_password", &[new_username.clone()]);
                let ah_ui = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_ui.upgrade() {
                        let app_typed: AppWindow = app;
                        app_typed.set_install_status(pw_step.clone().into());
                        let mut tb = app_typed.get_terminal_output().to_string();
                        tb.push_str(&format!("--> {}\n", pw_step));
                        app_typed.set_terminal_output(tb.into());
                    }
                });

                let pw_safe = new_user_password
                    .replace('\\', "\\\\")
                    .replace('$', "\\$")
                    .replace('`', "\\`");
                let pw_cmd = format!(
                    "/usr/sbin/chpasswd 2>/dev/null <<'PWEOF'\n{}:{}\nPWEOF\n",
                    new_username, pw_safe
                );
                let pw_args = ["-d", &final_name, "-u", "root", "--", "sh", "-c", &pw_cmd];
                let pw_result = executor.execute_command(&pw_args).await;

                if !pw_result.success {
                    // Retry with /sbin/chpasswd
                    let pw_cmd2 = format!(
                        "/sbin/chpasswd <<'PWEOF'\n{}:{}\nPWEOF\n",
                        new_username, pw_safe
                    );
                    let pw_args2 = ["-d", &final_name, "-u", "root", "--", "sh", "-c", &pw_cmd2];
                    let retry = executor.execute_command(&pw_args2).await;
                    if !retry.success {
                        let err = retry
                            .error
                            .unwrap_or_else(|| retry.output.trim().to_string());
                        let err = if err.is_empty() {
                            "chpasswd failed".to_string()
                        } else {
                            err
                        };
                        let fail_text = i18n::tr(
                            "install.step_set_user_password_failed",
                            &[new_username.clone(), err.clone()],
                        );
                        error_msg = fail_text.clone();
                        success = false;
                        let ah_ui = ah.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah_ui.upgrade() {
                                let app_typed: AppWindow = app;
                                app_typed.set_install_status(fail_text.clone().into());
                                let mut tb = app_typed.get_terminal_output().to_string();
                                tb.push_str(&format!("[FAIL] {}\n    {}\n", fail_text, err));
                                app_typed.set_terminal_output(tb.into());
                            }
                        });
                    }
                }

                if success {
                    let pw_done = i18n::tr(
                        "install.step_set_user_password_done",
                        &[new_username.clone()],
                    );
                    let ah_ui = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_ui.upgrade() {
                            let app_typed: AppWindow = app;
                            let mut tb = app_typed.get_terminal_output().to_string();
                            tb.push_str(&format!("      {}\n", pw_done));
                            app_typed.set_terminal_output(tb.into());
                        }
                    });
                }
            }

            // Set as default login user
            if success && set_default_user {
                let def_step = i18n::tr("install.step_set_default_user", &[new_username.clone()]);
                let ah_ui = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah_ui.upgrade() {
                        let app_typed: AppWindow = app;
                        app_typed.set_install_status(def_step.clone().into());
                        let mut tb = app_typed.get_terminal_output().to_string();
                        tb.push_str(&format!("--> {}\n", def_step));
                        app_typed.set_terminal_output(tb.into());
                    }
                });

                let uname = &new_username;
                let wsl_conf_cmd = format!(
                    "(grep -q '^default=' /etc/wsl.conf 2>/dev/null && sed -i 's/^default=.*/default={u}/' /etc/wsl.conf 2>/dev/null) || (grep -q '^\\[user\\]' /etc/wsl.conf 2>/dev/null && sed -i '/^\\[user\\]/a default={u}' /etc/wsl.conf 2>/dev/null) || printf '\\n[user]\\ndefault={u}\\n' >> /etc/wsl.conf",
                    u = uname
                );
                let def_args = [
                    "-d",
                    &final_name,
                    "-u",
                    "root",
                    "--",
                    "sh",
                    "-c",
                    &wsl_conf_cmd,
                ];
                let def_result = executor.execute_command(&def_args).await;

                if !def_result.success {
                    let err = def_result
                        .error
                        .unwrap_or_else(|| def_result.output.trim().to_string());
                    let err = if err.is_empty() {
                        "Failed to write /etc/wsl.conf".to_string()
                    } else {
                        err
                    };
                    let fail_text =
                        i18n::tr("install.step_set_default_user_failed", &[err.clone()]);
                    // Don't fail the whole install for this — just warn
                    let ah_ui = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_ui.upgrade() {
                            let app_typed: AppWindow = app;
                            let mut tb = app_typed.get_terminal_output().to_string();
                            tb.push_str(&format!("[WARN] {}\n    {}\n", fail_text, err));
                            app_typed.set_terminal_output(tb.into());
                        }
                    });
                } else {
                    let def_done = i18n::tr(
                        "install.step_set_default_user_done",
                        &[new_username.clone()],
                    );
                    let ah_ui = ah.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah_ui.upgrade() {
                            let app_typed: AppWindow = app;
                            let mut tb = app_typed.get_terminal_output().to_string();
                            tb.push_str(&format!("      {}\n", def_done));
                            app_typed.set_terminal_output(tb.into());
                        }
                    });
                }
            }
        }
    }

    let ah_final = ah.clone();
    let final_name_clone = final_name.clone();
    let error_msg_clone = error_msg.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = ah_final.upgrade() {
            let app_typed: AppWindow = app;
            if success {
                app_typed.set_install_success(true);
                app_typed.set_install_status(
                    i18n::tr("install.created_success", &[final_name_clone]).into(),
                );
            } else {
                app_typed.set_install_success(false);
                app_typed.set_install_status(
                    format!("{}: {}", i18n::t("install.error"), error_msg_clone).into(),
                );
            }
            app_typed.set_is_installing(false);
        }
    });

    // Force-terminate the distro after install to ensure clean state
    let _ = executor
        .execute_command(&["--terminate", &final_name])
        .await;
    info!("Terminated distro '{}' after install", final_name);

    if success {
        refresh_distros_ui(ah.clone(), as_ptr.clone()).await;
    }
}
