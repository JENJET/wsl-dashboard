use std::process::Stdio;
use tokio::io::{AsyncRead, AsyncReadExt};
// use tokio::task; // Removed
use tracing::{debug, error, info, warn};

use crate::wsl::models::WslCommandResult;

use crate::utils::system::{run_command_with_elevation, run_elevated_and_wait};
use crate::wsl::decoder::{WslOutputDecoder, decode_output};

const MAX_CONCURRENT: usize = 30;

/// Create a pre-configured async wsl.exe command with WSL_UTF8 and CREATE_NO_WINDOW.
pub fn new_tokio_wsl_cmd() -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("wsl.exe");
    cmd.env("WSL_UTF8", "1");
    #[cfg(windows)]
    cmd.creation_flags(crate::utils::system::CREATE_NO_WINDOW);
    cmd.kill_on_drop(true);
    cmd
}
const HEAVY_OP_TIMEOUT_SECS: u64 = 1800;
const WRITE_OP_TIMEOUT_SECS: u64 = 45;
const READ_OP_TIMEOUT_SECS: u64 = 10;
const SEMAPHORE_TIMEOUT_SECS: u64 = 10;
const STREAMING_TIMEOUT_SECS: u64 = 1800;
const MAX_OUTPUT_SIZE: usize = 1024 * 1024;
const READ_BUF_SIZE: usize = 8192;
const STREAM_BUF_SIZE: usize = 1024;

async fn read_limited_output<R>(mut reader: R, stream_name: &str) -> Result<Vec<u8>, String>
where
    R: AsyncRead + Unpin,
{
    let mut data = Vec::new();
    let mut buf = [0u8; READ_BUF_SIZE];

    loop {
        let n = reader
            .read(&mut buf)
            .await
            .map_err(|e| format!("{} read error: {}", stream_name, e))?;
        if n == 0 {
            break;
        }

        let remaining = MAX_OUTPUT_SIZE.saturating_sub(data.len());
        if remaining > 0 {
            data.extend_from_slice(&buf[..n.min(remaining)]);
        }
    }

    Ok(data)
}

// WSL command executor, responsible for executing various WSL commands
#[derive(Clone)]
pub struct WslCommandExecutor {
    // Limit concurrent WSL commands to prevent resource exhaustion
    semaphore: std::sync::Arc<tokio::sync::Semaphore>,
    // Semaphore to limit concurrent background heavy operations (like launcher cleanup)
    background_semaphore: std::sync::Arc<tokio::sync::Semaphore>,
}

impl Default for WslCommandExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl WslCommandExecutor {
    // Create a new WSL command executor instance
    pub fn new() -> Self {
        Self {
            // Limit to 30 concurrent operations. Higher than before to buffer hangs.
            semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT)),
            // Limit to 4 concurrent background heavy operations
            background_semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(4)),
        }
    }

    pub fn background_semaphore(&self) -> &std::sync::Arc<tokio::sync::Semaphore> {
        &self.background_semaphore
    }

    // Execute WSL commands asynchronously with elevation (UAC)
    pub async fn execute_command_elevated(&self, args: &[&str]) -> WslCommandResult<String> {
        self.execute_command_impl(args, true).await
    }

    // Execute WSL commands asynchronously
    pub async fn execute_command(&self, args: &[&str]) -> WslCommandResult<String> {
        self.execute_command_impl(args, false).await
    }

    async fn execute_command_impl(
        &self,
        args: &[&str],
        use_elevation: bool,
    ) -> WslCommandResult<String> {
        // Convert args to owned string vector for use in closure
        let args_owned: Vec<String> = args.iter().map(|&s| s.to_string()).collect();
        let command_str = format!("wsl {}", args_owned.join(" "));

        // Shared: write-op detection + logging
        let write_ops = [
            "--import",
            "--export",
            "--unregister",
            "--install",
            "--set-version",
            "--set-default-version",
            "--set-default",
            "-s",
            "--shutdown",
            "--terminate",
            "-t",
            "--mount",
            "--unmount",
            "--update",
            "--move",
            "--resize",
            "/bin/true",
        ];
        let is_write_op = args_owned.iter().any(|arg| {
            let lower = arg.to_lowercase();
            write_ops.contains(&lower.as_str())
                || lower.contains("useradd")
                || lower.contains("adduser")
                || lower.contains("chpasswd")
                || lower.contains("usermod")
                || lower.contains("passwd")
                || lower.contains("userdel")
        });
        if is_write_op {
            info!("Executing WSL command: {}", command_str);
        } else {
            debug!("Executing WSL command: {}", command_str);
        }
        if is_write_op {
            debug!("Starting async WSL command: {}", command_str);
        }

        // Shared: heavy-op detection + timeout
        let is_heavy_op = args_owned.iter().any(|arg| {
            let lower = arg.to_lowercase();
            matches!(
                lower.as_str(),
                "--import" | "--export" | "--install" | "--move" | "--resize" | "--update"
            )
        });
        let timeout_duration = if is_heavy_op {
            std::time::Duration::from_secs(HEAVY_OP_TIMEOUT_SECS)
        } else if is_write_op {
            std::time::Duration::from_secs(WRITE_OP_TIMEOUT_SECS)
        } else {
            std::time::Duration::from_secs(READ_OP_TIMEOUT_SECS)
        };
        if is_heavy_op {
            info!(
                "Executing heavy WSL operation with 30m timeout: {}",
                command_str
            );
        }

        // Shared: semaphore acquire
        let permit_timeout = std::time::Duration::from_secs(SEMAPHORE_TIMEOUT_SECS);
        debug!(
            "Acquiring WSL semaphore permit (Available: {}/{MAX_CONCURRENT}) for: {}",
            self.semaphore.available_permits(),
            command_str
        );
        let _permit = match tokio::time::timeout(permit_timeout, self.semaphore.acquire()).await {
            Ok(Ok(p)) => p,
            Ok(Err(_)) => {
                let err = "Failed to acquire semaphore permit (closed)".to_string();
                error!("{}", err);
                return WslCommandResult::error(String::new(), err);
            }
            Err(_) => {
                let err = format!(
                    "WSL command pending timeout after {}s (Queue full): {}",
                    permit_timeout.as_secs(),
                    command_str
                );
                warn!("{}", err);
                return WslCommandResult::error(String::new(), err);
            }
        };
        debug!("WSL semaphore permit acquired for: {}", command_str);

        let program = "wsl.exe".to_string();
        // Shared: unified output type for both paths
        let output = if use_elevation {
            info!("Executing WSL command with elevation: {}", command_str);
            let result = tokio::task::spawn_blocking(move || {
                run_command_with_elevation(&program, args_owned).map(|_| ())
            })
            .await
            .unwrap_or(Err("spawn_blocking failed".into()));
            match result {
                Ok(()) => (String::new(), String::new(), true),
                Err(e) => {
                    let err = e.to_string();
                    (String::new(), err.clone(), false)
                }
            }
        } else {
            let future = async {
                let mut cmd = crate::wsl::executor::new_tokio_wsl_cmd();
                cmd.args(&args_owned);
                cmd.stdout(Stdio::piped());
                cmd.stderr(Stdio::piped());

                debug!("Spawning wsl process for: {}", command_str);
                let mut child = cmd
                    .spawn()
                    .map_err(|e| format!("Failed to spawn wsl process: {}", e))?;
                debug!("Wsl process spawned (pid: {:?})", child.id());

                let stdout = child
                    .stdout
                    .take()
                    .ok_or_else(|| "Failed to capture stdout".to_string())?;
                let stderr = child
                    .stderr
                    .take()
                    .ok_or_else(|| "Failed to capture stderr".to_string())?;

                let (stdout_data, stderr_data) = tokio::try_join!(
                    read_limited_output(stdout, "Stdout"),
                    read_limited_output(stderr, "Stderr")
                )?;

                let status = child
                    .wait()
                    .await
                    .map_err(|e| format!("Failed to wait for child: {}", e))?;
                Ok::<(Vec<u8>, Vec<u8>, bool), String>((stdout_data, stderr_data, status.success()))
            };

            match tokio::time::timeout(timeout_duration, future).await {
                Ok(Ok((out, err, success))) => (decode_output(&out), decode_output(&err), success),
                Ok(Err(e)) => {
                    let error = format!("Command execution failed: {}", e);
                    error!("WSL command error: {}", error);
                    (String::new(), error, false)
                }
                Err(_) => {
                    let error = format!(
                        "WSL command timed out after {}s: {}",
                        timeout_duration.as_secs(),
                        command_str
                    );
                    error!("{}", error);
                    return {
                        drop(_permit);
                        WslCommandResult {
                            success: false,
                            output: String::new(),
                            error: Some(error),
                            data: None,
                            timeout: true,
                        }
                    };
                }
            }
        };

        let (stdout, stderr, success) = output;

        fn truncate_log(s: &str, max_len: usize) -> String {
            if s.len() > max_len {
                format!("{}... (truncated, total {} chars)", &s[..max_len], s.len())
            } else {
                s.to_string()
            }
        }
        if is_write_op {
            info!("WSL command stdout: {}", truncate_log(&stdout, 1000));
            if !stderr.is_empty() {
                info!("WSL command stderr: {}", truncate_log(&stderr, 1000));
            }
            info!("WSL command success: {}", success);
        } else {
            debug!("WSL command stdout: {}", truncate_log(&stdout, 1000));
            debug!("WSL command stderr: {}", truncate_log(&stderr, 1000));
            debug!("WSL command success: {}", success);
        }

        drop(_permit);
        if success {
            WslCommandResult::success(stdout, None)
        } else {
            let final_error = if stderr.trim().is_empty() && !stdout.trim().is_empty() {
                stdout.clone()
            } else {
                stderr
            };
            WslCommandResult::error(stdout, final_error)
        }
    }

    // Execute WSL commands asynchronously and callback output in real-time
    pub async fn execute_command_streaming<F>(
        &self,
        args: &[&str],
        mut callback: F,
    ) -> WslCommandResult<String>
    where
        F: FnMut(String) + Send + 'static,
    {
        let args_owned: Vec<String> = args.iter().map(|&s| s.to_string()).collect();
        let command_str = format!("wsl {}", args_owned.join(" "));
        info!("Executing Streaming WSL command: {}", command_str);

        let future = async {
            let mut cmd = crate::wsl::executor::new_tokio_wsl_cmd();
            cmd.args(&args_owned)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            let mut child = match cmd.spawn() {
                Ok(child) => {
                    info!("Process spawned successfully, PID: {:?}", child.id());
                    child
                }
                Err(e) => return Err(format!("Failed to spawn wsl: {}", e)),
            };

            let mut stdout = child.stdout.take().unwrap();
            let mut stderr = child.stderr.take().unwrap();

            let mut full_output = String::new();
            let mut stderr_output = String::new();
            let mut out_buf = [0u8; STREAM_BUF_SIZE];
            let mut err_buf = [0u8; STREAM_BUF_SIZE];

            let mut out_decoder = WslOutputDecoder::new();
            let mut err_decoder = WslOutputDecoder::new();

            let mut stdout_done = false;
            let mut stderr_done = false;

            // Wait for both process exit AND EOF on streams
            while !stdout_done || !stderr_done {
                tokio::select! {
                    result = stdout.read(&mut out_buf), if !stdout_done => {
                        match result {
                            Ok(0) => {
                                debug!("Streaming STDOUT reached EOF for: {}", command_str);
                                stdout_done = true;
                            }
                            Ok(n) => {
                                let text = out_decoder.decode(&out_buf[..n]);
                                if !text.is_empty() {
                                    full_output.push_str(&text);
                                    callback(text);
                                }
                            }
                            Err(e) => {
                                error!("Streaming STDOUT read error: {}", e);
                                stderr_output.push_str(&e.to_string());
                                stdout_done = true;
                            }
                        }
                    }
                    result = stderr.read(&mut err_buf), if !stderr_done => {
                        match result {
                            Ok(0) => {
                                debug!("Streaming STDERR reached EOF for: {}", command_str);
                                stderr_done = true;
                            }
                            Ok(n) => {
                                let text = err_decoder.decode(&err_buf[..n]);
                                full_output.push_str(&text);
                                //stderr_output.push_str(&text);
                                callback(text);
                            }
                            Err(e) => {
                                error!("Streaming STDERR read error: {}", e);
                                stderr_output.push_str(&e.to_string());
                                stderr_done = true;
                            }
                        }
                    }
                    // We don't exit early on process exit, we wait for EOF to get all data
                }
            }

            let status = child
                .wait()
                .await
                .map_err(|e| format!("Wait failed: {}", e))?;
            info!(
                "Streaming process exited with status: {} for: {}",
                status, command_str
            );

            Ok((full_output, stderr_output, status))
        };

        // Streaming commands usually used for install/import, so 30m timeout
        let timeout_duration = std::time::Duration::from_secs(STREAMING_TIMEOUT_SECS);

        // Also respect semaphore for consistency
        let permit_timeout = std::time::Duration::from_secs(10);
        let _permit = match tokio::time::timeout(permit_timeout, self.semaphore.acquire()).await {
            Ok(Ok(p)) => p,
            _ => {
                warn!(
                    "Streaming WSL command started without semaphore slot (too busy): {}",
                    command_str
                );
                // We proceed anyway but with a warning, or we could return error.
                // For install, it's better to fail early if WSL is totally unresponsive.
                return WslCommandResult::error(
                    String::new(),
                    "WSL service busy, please try again later".to_string(),
                );
            }
        };

        match tokio::time::timeout(timeout_duration, future).await {
            Ok(Ok((full_output, stderr_output, status))) => {
                if status.success() {
                    WslCommandResult::success(full_output, None)
                } else {
                    let err_msg = stderr_output.trim().to_string() + " " + &status.to_string();
                    WslCommandResult::error(full_output, err_msg)
                }
            }
            Ok(Err(e)) => {
                error!("Streaming command failed: {}", e);
                WslCommandResult::error(String::new(), e)
            }
            Err(_) => {
                let error = format!(
                    "Streaming WSL command timed out after {}s: {}",
                    timeout_duration.as_secs(),
                    command_str
                );
                error!("{}", error);
                WslCommandResult {
                    success: false,
                    output: String::new(),
                    error: Some(error),
                    data: None,
                    timeout: true,
                }
            }
        }
    }

    pub async fn check_path_exists(&self, distro_name: &str, path: &str) -> bool {
        if path == "~" {
            return true;
        }
        // wsl -d distro -e test -d path
        let result = self
            .execute_command(&["-d", distro_name, "-e", "test", "-d", path])
            .await;
        result.success
    }

    pub async fn check_file_executable(&self, distro_name: &str, path: &str) -> (bool, bool) {
        // Execute [ -f path ] to check if it's a file
        let exists_res = self
            .execute_command(&["-d", distro_name, "-u", "root", "-e", "test", "-f", path])
            .await;
        // Execute [ -x path ] to check if it's executable
        let exec_res = self
            .execute_command(&["-d", distro_name, "-u", "root", "-e", "test", "-x", path])
            .await;
        (exists_res.success, exec_res.success)
    }

    /// Execute a command with elevation in a console window.
    /// If `show_window` is true, the console window is visible (for user-facing operations).
    pub async fn execute_command_elevated_streaming<F>(
        &self,
        program: &str,
        args: &[&str],
        show_window: bool,
        _callback: F,
    ) -> WslCommandResult<String>
    where
        F: FnMut(String) + Send + 'static,
    {
        let args_str = args.join(" ");
        let command_str = format!("{} {}", program, args_str);
        info!(
            "Executing elevated command (visible window): {}",
            command_str
        );
        let cmd_str = command_str.clone();

        let (prog, cmd_args) = if program == "cmd.exe" {
            // Already a cmd.exe command, pass through directly (avoid double wrapping)
            (
                "cmd.exe".to_string(),
                args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            )
        } else {
            // Wrap non-cmd commands with cmd.exe /c for console window behavior
            let cmd_code = format!("{} {}", program, args_str);
            ("cmd.exe".to_string(), vec!["/c".to_string(), cmd_code])
        };

        let (tx, rx) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            let result = match run_elevated_and_wait(&prog, cmd_args, show_window, None) {
                Ok(0) => {
                    info!("Elevated command succeeded: {}", command_str);
                    WslCommandResult::success(String::new(), None)
                }
                Ok(3010) => {
                    // 3010 = ERROR_SUCCESS_REBOOT_REQUIRED (DISM success with pending reboot)
                    info!(
                        "Elevated command succeeded (reboot required): {}",
                        command_str
                    );
                    WslCommandResult::success("REBOOT_REQUIRED".to_string(), None)
                }
                Ok(code) => {
                    let err = format!("exit code: {}", code);
                    error!("Elevated command failed: {} - {}", command_str, err);
                    WslCommandResult::error(String::new(), err)
                }
                Err(e) => {
                    error!("Elevated command error: {} - {}", command_str, e);
                    WslCommandResult::error(String::new(), e)
                }
            };
            let _ = tx.send(result);
        });

        rx.await.unwrap_or_else(|_| {
            error!("Elevated command task panicked: {}", cmd_str);
            WslCommandResult::error(String::new(), "Task panicked".to_string())
        })
    }
}
