use crate::i18n;
use crate::utils::system;
use crate::wsl::ops::vhdx;
use crate::{AppState, AppWindow};
use slint::Weak;
use std::process::Command as StdCommand;
use std::sync::Arc;
use tokio::sync::Mutex;

pub fn setup(app: &AppWindow, app_handle: Weak<AppWindow>, _app_state: Arc<Mutex<AppState>>) {
    let ah = app_handle.clone();
    app.on_vhdx_compact_show(move |distro_name, vhdx_path, vhdx_size| {
        let ah = ah.clone();
        let name = distro_name.to_string();
        let vpath = vhdx_path.to_string();
        let vsize = vhdx_size.to_string();

        if let Some(app) = ah.upgrade() {
            app.set_vhdx_compact_distro_name(name.clone().into());
            app.set_vhdx_compact_vhdx_path(vpath.into());
            app.set_vhdx_compact_vhdx_size(vsize.into());
            app.set_vhdx_compact_output("".into());
            app.set_vhdx_compact_is_error(false);
            app.set_vhdx_compact_running(false);
            app.set_vhdx_compact_clean_cache(true);
            app.set_vhdx_compact_backup_tar(true);
            app.set_vhdx_compact_compacted(false);

            let vhdx_path_str = app.get_vhdx_compact_vhdx_path().to_string();
            if let Some(info) = vhdx::get_vhdx_info(&vhdx_path_str) {
                app.set_vhdx_compact_is_sparse(info.is_sparse);
            }

            let vhdx_dir = std::path::Path::new(&vhdx_path_str)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let default_backup = format!("{}\\{}.ext4.tar", vhdx_dir, name);
            app.set_vhdx_compact_backup_path(default_backup.into());
            update_drive_info(&ah, &vhdx_path_str);
            check_backup_path_exists(&ah);
            app.set_show_vhdx_compact(true);
        }
    });

    let ah = app_handle.clone();
    app.on_vhdx_compact_cancel(move || {
        if let Some(app) = ah.upgrade() {
            app.set_show_vhdx_compact(false);
        }
    });

    let ah = app_handle.clone();
    let state_for_close = _app_state.clone();
    app.on_vhdx_compact_close(move || {
        let should_refresh = if let Some(app) = ah.upgrade() {
            let compacted = app.get_vhdx_compact_compacted();
            app.set_show_vhdx_compact(false);
            app.set_vhdx_compact_output("".into());
            compacted
        } else {
            false
        };
        if should_refresh {
            let ah = ah.clone();
            let state = state_for_close.clone();
            tokio::spawn(async move {
                crate::ui::data::refresh_distros_ui(ah, state).await;
            });
        }
    });

    let ah = app_handle.clone();
    app.on_vhdx_compact_select_backup_path(move || {
        let ah = ah.clone();
        let (default_path, distro_name, vhdx_path) = {
            if let Some(app) = ah.upgrade() {
                (
                    app.get_vhdx_compact_backup_path().to_string(),
                    app.get_vhdx_compact_distro_name().to_string(),
                    app.get_vhdx_compact_vhdx_path().to_string(),
                )
            } else {
                return;
            }
        };

        tokio::task::spawn_blocking(move || {
            let dialog = rfd::FileDialog::new()
                .set_title("Select backup path")
                .add_filter("TAR Files", &["tar"])
                .add_filter("All Files", &["*"]);

            let dialog = if !default_path.is_empty() {
                if let Some(dir) = std::path::Path::new(&default_path).parent() {
                    dialog.set_directory(dir)
                } else {
                    dialog
                }
            } else if !vhdx_path.is_empty() {
                if let Some(dir) = std::path::Path::new(&vhdx_path).parent() {
                    dialog.set_directory(dir)
                } else {
                    dialog
                }
            } else {
                dialog
            };

            let file_name = if !default_path.is_empty() {
                std::path::Path::new(&default_path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| format!("{}.ext4.tar", distro_name))
            } else if !distro_name.is_empty() {
                format!("{}.ext4.tar", distro_name)
            } else {
                "backup.tar".to_string()
            };
            let dialog = dialog.set_file_name(&file_name);

            if let Some(selected) = dialog.save_file() {
                let path_str = selected.to_string_lossy().to_string();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah.upgrade() {
                        app.set_vhdx_compact_backup_path(path_str.into());
                        let vpath = app.get_vhdx_compact_vhdx_path().to_string();
                        update_drive_info(&ah, &vpath);
                        check_backup_path_exists(&ah);
                    }
                });
            }
        });
    });

    let ah = app_handle.clone();
    app.on_vhdx_compact_backup_path_changed(move |_path| {
        if let Some(app) = ah.upgrade() {
            let vpath = app.get_vhdx_compact_vhdx_path().to_string();
            update_drive_info(&ah, &vpath);
            check_backup_path_exists(&ah);
        }
    });

    let ah_outer = app_handle.clone();
    app.on_vhdx_compact_execute(move || {
        let ah = ah_outer.clone();

        let distro_name = {
            if let Some(app) = ah.upgrade() {
                app.get_vhdx_compact_distro_name().to_string()
            } else {
                return;
            }
        };

        let clean_cache = {
            if let Some(app) = ah.upgrade() {
                app.get_vhdx_compact_clean_cache()
            } else {
                return;
            }
        };

        let backup_tar = {
            if let Some(app) = ah.upgrade() {
                app.get_vhdx_compact_backup_tar()
            } else {
                return;
            }
        };

        let vhdx_path = {
            if let Some(app) = ah.upgrade() {
                app.get_vhdx_compact_vhdx_path().to_string()
            } else {
                return;
            }
        };

        let backup_path = {
            if let Some(app) = ah.upgrade() {
                app.get_vhdx_compact_backup_path().to_string()
            } else {
                return;
            }
        };

        let is_sparse = {
            if let Some(app) = ah.upgrade() {
                app.get_vhdx_compact_is_sparse()
            } else {
                false
            }
        };

        let initial_size = vhdx::get_vhdx_file_size(&vhdx_path);

        {
            if let Some(app) = ah.upgrade() {
                app.set_vhdx_compact_running(true);
                app.set_vhdx_compact_is_error(false);
                app.set_vhdx_compact_output("".into());
            }
        }

        tokio::spawn(async move {
            let append_output = |ah: &Weak<AppWindow>, text: String| {
                let ah = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah.upgrade() {
                        let current = app.get_vhdx_compact_output().to_string();
                        app.set_vhdx_compact_output((current + &text).into());
                    }
                });
            };

            let set_error = |ah: &Weak<AppWindow>| {
                let ah = ah.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ah.upgrade() {
                        app.set_vhdx_compact_is_error(true);
                        app.set_vhdx_compact_running(false);
                    }
                });
            };

            let set_done = |ah: &Weak<AppWindow>| {
                let ah = ah.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ah.upgrade() {
                            app.set_vhdx_compact_running(false);
                        }
                    });
                });
            };

            let dn = &distro_name;
            let mut step_num = 0u32;
            let mut total_steps = 0u32;
            if clean_cache { total_steps += 1; }
            total_steps += 1; // fstrim
            total_steps += 1; // stop
            if backup_tar { total_steps += 1; }
            total_steps += 1; // shutdown
            if is_sparse { total_steps += 1; } // unset sparse
            total_steps += 1; // compact
            if is_sparse { total_steps += 1; } // restore sparse

            let mut run_step = |step_key: &str, cmd: &str, args: &[&str], fmt_args: &[String], timeout_secs: Option<u64>| -> bool {
                step_num += 1;
                let label_text = if fmt_args.is_empty() {
                    i18n::t(step_key)
                } else {
                    i18n::tr(step_key, fmt_args)
                };
                let label = i18n::tr("dialog.vhdx_compact_step", &[
                    step_num.to_string(),
                    total_steps.to_string(),
                    label_text,
                ]);
                append_output(&ah, format!("{}\n", label));

                let cmd_display = format!("{} {}", cmd, args.join(" "));
                append_output(&ah, format!("  {}\n", i18n::tr("dialog.vhdx_compact_cmd", &[cmd_display])));

                let result = match timeout_secs {
                    Some(secs) => {
                        use std::io::Read;
                        use std::process::Stdio;
                        use std::time::{Duration, Instant};

                        let mut child = match StdCommand::new(cmd)
                            .args(args)
                            .stdout(Stdio::piped())
                            .stderr(Stdio::piped())
                            .spawn()
                        {
                            Ok(c) => c,
                            Err(e) => {
                                append_output(&ah, format!("{}\n", e));
                                return false;
                            }
                        };

                        let deadline = Instant::now() + Duration::from_secs(secs);
                        loop {
                            match child.try_wait() {
                                Ok(Some(status)) => {
                                    let mut stdout = Vec::new();
                                    let mut stderr = Vec::new();
                                    if let Some(mut out) = child.stdout.take() {
                                        let _ = out.read_to_end(&mut stdout);
                                    }
                                    if let Some(mut out) = child.stderr.take() {
                                        let _ = out.read_to_end(&mut stderr);
                                    }
                                    break Ok((status, stdout, stderr));
                                }
                                Ok(None) => {
                                    if Instant::now() >= deadline {
                                        let _ = child.kill();
                                        let _ = child.wait();
                                        break Err("timeout".to_string());
                                    }
                                    std::thread::sleep(Duration::from_millis(100));
                                }
                                Err(e) => {
                                    break Err(e.to_string());
                                }
                            }
                        }
                    }
                    None => {
                        match StdCommand::new(cmd).args(args).output() {
                            Ok(out) => Ok((out.status, out.stdout, out.stderr)),
                            Err(e) => Err(e.to_string()),
                        }
                    }
                };

                match result {
                    Ok((status, stdout, stderr)) => {
                        if !stdout.is_empty() {
                            if let Ok(s) = String::from_utf8(stdout) {
                                append_output(&ah, s);
                            }
                        }
                        if !stderr.is_empty() {
                            if let Ok(s) = String::from_utf8(stderr) {
                                append_output(&ah, s);
                            }
                        }
                        if status.success() {
                            let code = status.code().unwrap_or(0);
                            append_output(&ah, format!("  {}\n", i18n::tr("dialog.vhdx_compact_cmd_success", &[code.to_string()])));
                            true
                        } else {
                            let code = status.code().unwrap_or(-1);
                            append_output(&ah, format!("  {}\n", i18n::tr("dialog.vhdx_compact_cmd_failed", &[code.to_string()])));
                            false
                        }
                    }
                    Err(e) => {
                        append_output(&ah, format!("  {}\n", i18n::tr("dialog.vhdx_compact_cmd_failed", &[e])));
                        false
                    }
                }
            };

            // 1. clean cache (optional)
            if clean_cache {
                if !run_step(
                    "dialog.vhdx_compact_running_clean",
                    "wsl",
                    &["-d", dn, "-u", "root", "--", "/bin/sh", "-c",
                      "apt-get clean 2>/dev/null; apt-get autoremove -y 2>/dev/null; dnf clean all 2>/dev/null; yum clean all 2>/dev/null; pacman -Sc --noconfirm 2>/dev/null; zypper clean 2>/dev/null; rm -rf /tmp/* /var/tmp/* /var/cache/man/* /root/.cache/* 2>/dev/null; journalctl --vacuum-time=3d 2>/dev/null"],
                    &[],
                    Some(60),
                ) {
                    set_error(&ah);
                    goto_restore_sparse(&ah, dn, is_sparse, &mut step_num, total_steps, &append_output, &set_done);
                    return;
                }
            }

            // 2. fstrim
            if !run_step("dialog.vhdx_compact_running_fstrim", "wsl", &["-d", dn, "-u", "root", "--", "fstrim", "-av"], &[], Some(60)) {
                set_error(&ah);
                goto_restore_sparse(&ah, dn, is_sparse, &mut step_num, total_steps, &append_output, &set_done);
                return;
            }

            // 3. stop
            if !run_step("dialog.vhdx_compact_running_stop", "wsl", &["-t", dn], &[dn.to_string()], Some(60)) {
                set_error(&ah);
                goto_restore_sparse(&ah, dn, is_sparse, &mut step_num, total_steps, &append_output, &set_done);
                return;
            }

            // 4. backup (optional)
            if backup_tar {
                if !run_step("dialog.vhdx_compact_running_backup", "wsl", &["--export", dn, &backup_path], &[backup_path.to_string()], None) {
                    set_error(&ah);
                    goto_restore_sparse(&ah, dn, is_sparse, &mut step_num, total_steps, &append_output, &set_done);
                    return;
                }
            }

            // 5. shutdown
            if !run_step("dialog.vhdx_compact_running_shutdown", "wsl", &["--shutdown"], &[], Some(30)) {
                set_error(&ah);
                goto_restore_sparse(&ah, dn, is_sparse, &mut step_num, total_steps, &append_output, &set_done);
                return;
            }

            // 6. unset sparse (if applicable)
            if is_sparse {
                if !run_step("dialog.vhdx_compact_running_unset_sparse", "wsl", &["--manage", dn, "--set-sparse", "false"], &[], Some(60)) {
                    set_error(&ah);
                    goto_restore_sparse(&ah, dn, is_sparse, &mut step_num, total_steps, &append_output, &set_done);
                    return;
                }
            }

            // 7. compact (needs elevation)
            step_num += 1;
            let compact_label = i18n::tr("dialog.vhdx_compact_step", &[
                step_num.to_string(),
                total_steps.to_string(),
                i18n::t("dialog.vhdx_compact_running_compact"),
            ]);
            append_output(&ah, format!("{}\n", compact_label));

            let script_dir = std::env::temp_dir().join("wd");
            let _ = std::fs::create_dir_all(&script_dir);
            let script_path = script_dir.join(format!("diskpart_{}.txt", std::process::id()));

            let script_content = format!(
                "select vdisk file=\"{}\"\ncompact vdisk\nexit\n",
                vhdx_path
            );

            if let Err(e) = std::fs::write(&script_path, &script_content) {
                append_output(&ah, format!("{}\n", e));
                set_error(&ah);
                goto_restore_sparse(&ah, dn, is_sparse, &mut step_num, total_steps, &append_output, &set_done);
                return;
            }

            let diskpart_result = system::run_elevated_and_wait(
                "diskpart.exe",
                vec!["/s".to_string(), script_path.to_string_lossy().to_string()],
                false,
                None,
            );

            let _ = std::fs::remove_file(&script_path);

            match diskpart_result {
                Ok(code) if code == 0 => {
                    let final_size = vhdx::get_vhdx_file_size(&vhdx_path);
                    append_output(&ah, format!("  {}\n", i18n::t("dialog.vhdx_compact_success")));

                    // Mark compact as successful for refresh on close
                    {
                        let ah = ah.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah.upgrade() {
                                app.set_vhdx_compact_compacted(true);
                            }
                        });
                    }

                    if let (Some(before), Some(after)) = (initial_size, final_size) {
                        let before_gb = before as f64 / (1024.0 * 1024.0 * 1024.0);
                        let after_gb = after as f64 / (1024.0 * 1024.0 * 1024.0);
                        let reduced = if before > after {
                            let diff = before - after;
                            let diff_gb = diff as f64 / (1024.0 * 1024.0 * 1024.0);
                            format!("{:.2} GB", diff_gb)
                        } else {
                            "0 B".to_string()
                        };
                        let size_msg = i18n::tr(
                            "dialog.vhdx_compact_success_size",
                            &[format!("{:.2} GB", before_gb), format!("{:.2} GB", after_gb), reduced],
                        );
                        append_output(&ah, format!("  {}\n", size_msg));

                        let ah2 = ah.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = ah2.upgrade() {
                                app.set_vhdx_compact_vhdx_size(format!("{:.2} GB", after_gb).into());
                                let mut info = app.get_information();
                                info.vhdx_size = format!("{:.2} GB", after_gb).into();
                                app.set_information(info);
                            }
                        });
                    }

                    // restore sparse after successful compact
                    if is_sparse {
                        step_num += 1;
                        let restore_text = i18n::t("dialog.vhdx_compact_running_restore_sparse");
                        let restore_label = i18n::tr("dialog.vhdx_compact_step", &[
                            step_num.to_string(),
                            total_steps.to_string(),
                            restore_text,
                        ]);
                        append_output(&ah, format!("{}\n", restore_label));

                        let cmd_display = format!("wsl --manage {} --set-sparse true --allow-unsafe", dn);
                        append_output(&ah, format!("  {}\n", i18n::tr("dialog.vhdx_compact_cmd", &[cmd_display])));

                        use std::io::Read;
                        use std::process::Stdio;
                        use std::time::{Duration, Instant};

                        let mut child = match crate::utils::system::new_wsl_command()
                            .args(["--manage", dn, "--set-sparse", "true", "--allow-unsafe"])
                            .stdout(Stdio::piped())
                            .stderr(Stdio::piped())
                            .spawn()
                        {
                            Ok(c) => c,
                            Err(e) => {
                                append_output(&ah, format!("{}\n", e));
                                set_done(&ah);
                                return;
                            }
                        };

                        let deadline = Instant::now() + Duration::from_secs(60);
                        loop {
                            match child.try_wait() {
                                Ok(Some(status)) => {
                                    let mut stdout = Vec::new();
                                    let mut stderr = Vec::new();
                                    if let Some(mut out) = child.stdout.take() {
                                        let _ = out.read_to_end(&mut stdout);
                                    }
                                    if let Some(mut out) = child.stderr.take() {
                                        let _ = out.read_to_end(&mut stderr);
                                    }
                                    if !stdout.is_empty() {
                                        if let Ok(s) = String::from_utf8(stdout) {
                                            if !s.is_empty() { append_output(&ah, s); }
                                        }
                                    }
                                    if !stderr.is_empty() {
                                        if let Ok(s) = String::from_utf8(stderr) {
                                            if !s.is_empty() { append_output(&ah, s); }
                                        }
                                    }
                                    let code = status.code().unwrap_or(0);
                                    if status.success() {
                                        append_output(&ah, format!("  {}\n", i18n::tr("dialog.vhdx_compact_cmd_success", &[code.to_string()])));
                                    } else {
                                        append_output(&ah, format!("  {}\n", i18n::tr("dialog.vhdx_compact_cmd_failed", &[code.to_string()])));
                                    }
                                    break;
                                }
                                Ok(None) => {
                                    if Instant::now() >= deadline {
                                        let _ = child.kill();
                                        let _ = child.wait();
                                        append_output(&ah, format!("  {}\n", i18n::tr("dialog.vhdx_compact_cmd_failed", &["timeout".to_string()])));
                                        break;
                                    }
                                    std::thread::sleep(Duration::from_millis(100));
                                }
                                Err(e) => {
                                    append_output(&ah, format!("{}\n", e));
                                    break;
                                }
                            }
                        }
                    }
                    set_done(&ah);
                }
                Ok(code) => {
                    append_output(&ah, format!("  {}\n", i18n::tr("dialog.vhdx_compact_failed", &[format!("exit code: {}", code)])));
                    set_error(&ah);
                    goto_restore_sparse(&ah, dn, is_sparse, &mut step_num, total_steps, &append_output, &set_done);
                }
                Err(e) => {
                    append_output(&ah, format!("  {}\n", i18n::tr("dialog.vhdx_compact_failed", &[e])));
                    set_error(&ah);
                    goto_restore_sparse(&ah, dn, is_sparse, &mut step_num, total_steps, &append_output, &set_done);
                }
            }
        });
    });
}

fn goto_restore_sparse(
    ah: &Weak<AppWindow>,
    distro_name: &str,
    is_sparse: bool,
    step_num: &mut u32,
    total_steps: u32,
    append_output: &dyn Fn(&Weak<AppWindow>, String),
    set_done: &dyn Fn(&Weak<AppWindow>),
) {
    if is_sparse {
        *step_num += 1;
        let restore_text = i18n::t("dialog.vhdx_compact_running_restore_sparse");
        let restore_label = i18n::tr(
            "dialog.vhdx_compact_step",
            &[step_num.to_string(), total_steps.to_string(), restore_text],
        );
        append_output(ah, format!("{}\n", restore_label));

        let cmd_display = format!(
            "wsl --manage {} --set-sparse true --allow-unsafe",
            distro_name
        );
        append_output(
            ah,
            format!(
                "  {}\n",
                i18n::tr("dialog.vhdx_compact_cmd", &[cmd_display])
            ),
        );

        use std::io::Read;
        use std::process::Stdio;
        use std::time::{Duration, Instant};

        let result = {
            let mut child = match crate::utils::system::new_wsl_command()
                .args([
                    "--manage",
                    distro_name,
                    "--set-sparse",
                    "true",
                    "--allow-unsafe",
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    append_output(ah, format!("{}\n", e));
                    set_done(ah);
                    return;
                }
            };

            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        let mut stdout = Vec::new();
                        let mut stderr = Vec::new();
                        if let Some(mut out) = child.stdout.take() {
                            let _ = out.read_to_end(&mut stdout);
                        }
                        if let Some(mut out) = child.stderr.take() {
                            let _ = out.read_to_end(&mut stderr);
                        }
                        break (status, stdout, stderr);
                    }
                    Ok(None) => {
                        if Instant::now() >= deadline {
                            let _ = child.kill();
                            let _ = child.wait();
                            append_output(
                                ah,
                                format!(
                                    "  {}\n",
                                    i18n::tr(
                                        "dialog.vhdx_compact_cmd_failed",
                                        &["timeout".to_string()]
                                    )
                                ),
                            );
                            set_done(ah);
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    Err(e) => {
                        append_output(ah, format!("{}\n", e));
                        set_done(ah);
                        return;
                    }
                }
            }
        };

        if !result.1.is_empty() {
            if let Ok(s) = String::from_utf8(result.1) {
                if !s.is_empty() {
                    append_output(ah, s);
                }
            }
        }
        if !result.2.is_empty() {
            if let Ok(s) = String::from_utf8(result.2) {
                if !s.is_empty() {
                    append_output(ah, s);
                }
            }
        }
        let code = result.0.code().unwrap_or(0);
        if result.0.success() {
            append_output(
                ah,
                format!(
                    "  {}\n",
                    i18n::tr("dialog.vhdx_compact_cmd_success", &[code.to_string()])
                ),
            );
        } else {
            append_output(
                ah,
                format!(
                    "  {}\n",
                    i18n::tr("dialog.vhdx_compact_cmd_failed", &[code.to_string()])
                ),
            );
        }
    }
    set_done(ah);
}

fn check_backup_path_exists(ah: &Weak<AppWindow>) {
    if let Some(app) = ah.upgrade() {
        let path = app.get_vhdx_compact_backup_path().to_string();
        let exists = std::path::Path::new(&path).exists();
        app.set_vhdx_compact_backup_exists(exists);
    }
}

fn update_drive_info(ah: &Weak<AppWindow>, _vhdx_path: &str) {
    if let Some(app) = ah.upgrade() {
        let backup_path = app.get_vhdx_compact_backup_path().to_string();
        let drive_root = crate::utils::system::get_drive_root(&backup_path);
        let disk_info = system::get_disk_space(&drive_root);
        let free_gb = disk_info.unused_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        let total_gb = disk_info.total_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        let free_str = format!("{:.2} GB", free_gb);
        let total_str = format!("{:.2} GB", total_gb);

        app.set_vhdx_compact_drive_path(drive_root.into());
        app.set_vhdx_compact_drive_free(free_str.into());
        app.set_vhdx_compact_drive_total(total_str.into());

        let sufficient = disk_info.unused_bytes > 1024 * 1024 * 1024;
        app.set_vhdx_compact_drive_sufficient(sufficient);
    }
}
