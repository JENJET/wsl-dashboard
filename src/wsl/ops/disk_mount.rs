use crate::utils::system::CREATE_NO_WINDOW;
use crate::wsl::executor::WslCommandExecutor;
use crate::wsl::models::{PhysicalDisk, WslCommandResult};
use std::os::windows::process::CommandExt;
use tracing::info;

/// List physical disks via PowerShell Get-Disk
pub async fn list_physical_disks() -> WslCommandResult<Vec<PhysicalDisk>> {
    let output = tokio::task::spawn_blocking(|| {
        std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Get-Disk | Select-Object Number, FriendlyName, Size, BusType, PartitionStyle | ForEach-Object { \"$($_.Number)|$($_.FriendlyName)|$($_.Size)|$($_.BusType)|$($_.PartitionStyle)\" }",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    })
    .await
    .unwrap_or(Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "spawn_blocking failed",
    )));

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                return WslCommandResult::error(stdout, stderr);
            }
            let disks = parse_physical_disks(&stdout);
            WslCommandResult::success(stdout, Some(disks))
        }
        Err(e) => WslCommandResult::error(
            String::new(),
            format!("Failed to list physical disks: {}", e),
        ),
    }
}

fn parse_physical_disks(output: &str) -> Vec<PhysicalDisk> {
    let mut disks = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() < 5 {
            continue;
        }
        let number = match parts[0].trim().parse::<u32>() {
            Ok(n) => n,
            Err(_) => continue,
        };
        let friendly_name = parts[1].trim().to_string();
        let size_bytes = parts[2].trim().parse::<u64>().unwrap_or(0);
        let bus_type = parts[3].trim().to_string();
        let partition_style = parts[4].trim().to_string();

        disks.push(PhysicalDisk {
            number,
            friendly_name,
            size: format_size(size_bytes),
            size_bytes,
            bus_type,
            partition_style,
        });
    }
    disks
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 * 1024 {
        format!(
            "{:.2} TB",
            bytes as f64 / (1024.0 * 1024.0 * 1024.0 * 1024.0)
        )
    } else if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Build the wsl.exe command line for mounting (for elevated fallback)
fn build_mount_cmd(
    disk: &str,
    is_vhd: bool,
    is_bare: bool,
    name: &str,
    fs_type: &str,
    partition: &str,
    options: &str,
) -> String {
    let mut cmd = format!("wsl.exe --mount \"{}\"", disk);
    if is_vhd {
        cmd.push_str(" --vhd");
    }
    if is_bare {
        cmd.push_str(" --bare");
    }
    if !name.is_empty() {
        cmd.push_str(&format!(" --name \"{}\"", name));
    }
    if !fs_type.is_empty() {
        cmd.push_str(&format!(" --type {}", fs_type));
    }
    if !partition.is_empty() {
        cmd.push_str(&format!(" --partition {}", partition));
    }
    if !options.is_empty() {
        cmd.push_str(&format!(" --options \"{}\"", options));
    }
    cmd
}

/// Run wsl.exe elevated via a temporary batch file, capturing stdout+stderr to a temp file.
/// This avoids cmd.exe /c command line quoting issues with ShellExecuteExW.
pub async fn mount_disk_elevated(
    disk: &str,
    is_vhd: bool,
    is_bare: bool,
    name: &str,
    fs_type: &str,
    partition: &str,
    options: &str,
) -> WslCommandResult<String> {
    let cmd = build_mount_cmd(disk, is_vhd, is_bare, name, fs_type, partition, options);
    info!("Mounting disk (elevated fallback): {}", cmd);
    run_elevated_wsl_with_output(&cmd).await
}

/// Mount a disk into WSL2 (non-elevated first attempt)
pub async fn mount_disk(
    executor: &WslCommandExecutor,
    disk: &str,
    is_vhd: bool,
    is_bare: bool,
    name: &str,
    fs_type: &str,
    partition: &str,
    options: &str,
) -> WslCommandResult<String> {
    let mut args: Vec<&str> = vec!["--mount", disk];

    if is_vhd {
        args.push("--vhd");
    }
    if is_bare {
        args.push("--bare");
    }
    if !name.is_empty() {
        args.push("--name");
        args.push(name);
    }
    if !fs_type.is_empty() {
        args.push("--type");
        args.push(fs_type);
    }
    if !partition.is_empty() {
        args.push("--partition");
        args.push(partition);
    }
    if !options.is_empty() {
        args.push("--options");
        args.push(options);
    }

    info!("Mounting disk: {} (vhd={}, bare={})", disk, is_vhd, is_bare);
    executor.execute_command(&args).await
}

/// Unmount a disk (or all disks if disk is empty) — non-elevated first attempt
pub async fn unmount_disk(executor: &WslCommandExecutor, disk: &str) -> WslCommandResult<String> {
    let args: Vec<&str> = if disk.is_empty() {
        vec!["--unmount"]
    } else {
        vec!["--unmount", disk]
    };

    info!(
        "Unmounting disk: {}",
        if disk.is_empty() { "all" } else { disk }
    );
    executor.execute_command(&args).await
}

async fn run_elevated_wsl_with_output(cmdline: &str) -> WslCommandResult<String> {
    let cmd_owned = cmdline.to_string();
    tokio::task::spawn_blocking(move || {
        let tmp_dir = std::env::temp_dir();
        let log_file = tmp_dir.join(format!("wsl_elevated_{}.log", std::process::id()));
        let bat_file = tmp_dir.join(format!("wsl_elevated_{}.bat", std::process::id()));

        let log_path = log_file.display().to_string();
        let bat_content = format!("@{} >\"{}\" 2>&1\r\n", cmd_owned, log_path);
        info!("Batch file content: {}", bat_content.trim());
        if let Err(e) = std::fs::write(&bat_file, &bat_content) {
            let _ = std::fs::remove_file(&bat_file);
            return WslCommandResult::error(
                String::new(),
                format!("Failed to create temp batch file: {}", e),
            );
        }

        let bat_path = bat_file.display().to_string();
        info!("Running elevated: cmd.exe /c {}", bat_path);
        let exit_code = crate::utils::system::run_elevated_and_wait(
            "cmd.exe",
            vec!["/c".to_string(), bat_path],
            false,
            None,
        );

        let _ = std::fs::remove_file(&bat_file);
        let captured = std::fs::read_to_string(&log_file).unwrap_or_default();
        let _ = std::fs::remove_file(&log_file);
        let clean = captured.trim().to_string();
        info!(
            "Elevated wsl exit_code: {:?}, output length: {}, output: {:?}",
            exit_code,
            clean.len(),
            &clean[..std::cmp::min(clean.len(), 500)]
        );

        match exit_code {
            Ok(code) if code == 0 => WslCommandResult::success(clean, None),
            Ok(code) => {
                let err = if clean.is_empty() {
                    format!("Process exited with code {}", code)
                } else {
                    clean
                };
                WslCommandResult::error(String::new(), err)
            }
            Err(e) => {
                let err = if !clean.is_empty() {
                    clean
                } else {
                    format!("Elevation failed: {}", e)
                };
                WslCommandResult::error(String::new(), err)
            }
        }
    })
    .await
    .unwrap_or_else(|e| {
        WslCommandResult::error(String::new(), format!("spawn_blocking failed: {}", e))
    })
}
