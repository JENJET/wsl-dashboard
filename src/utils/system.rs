use std::process::{Command, Output};
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::core::HSTRING;

use crate::i18n;

pub const CREATE_NO_WINDOW: u32 = 0x08000000;
pub const CREATE_NEW_CONSOLE: u32 = 0x00000010;

/// Extract drive root (e.g. "C:\\") from a path, handling "\\\\?\\" prefix.
pub fn get_drive_root(path: &str) -> String {
    if path.len() >= 7 && path.starts_with("\\\\?\\") {
        // "\\?\C:\..." → "C:\"
        path[4..7].to_string()
    } else if path.len() >= 3 && path.as_bytes()[1] == b':' {
        // "C:\..." → "C:\"
        path[..3].to_string()
    } else {
        "C:\\".to_string()
    }
}

/// Create a pre-configured powershell.exe command with common flags.
pub fn run_powershell(script: &str) -> Result<Output, std::io::Error> {
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.output()
}

/// Create a pre-configured wsl.exe command with WSL_UTF8 and CREATE_NO_WINDOW.
pub fn new_wsl_command() -> Command {
    let mut cmd = Command::new("wsl");
    cmd.env("WSL_UTF8", "1");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

pub struct DiskSpaceInfo {
    pub total_bytes: u64,
    pub unused_bytes: u64,
    #[allow(dead_code)]
    pub used_bytes: u64,
}

pub fn get_disk_space(path: &str) -> DiskSpaceInfo {
    let mut free_bytes_available: u64 = 0;
    let mut total_number_of_bytes: u64 = 0;
    let mut total_number_of_free_bytes: u64 = 0;

    let path_hstring = HSTRING::from(path);
    unsafe {
        if GetDiskFreeSpaceExW(
            &path_hstring,
            Some(&mut free_bytes_available),
            Some(&mut total_number_of_bytes),
            Some(&mut total_number_of_free_bytes),
        )
        .is_ok()
        {
            DiskSpaceInfo {
                total_bytes: total_number_of_bytes,
                unused_bytes: free_bytes_available,
                used_bytes: total_number_of_bytes - free_bytes_available,
            }
        } else {
            DiskSpaceInfo {
                total_bytes: 0,
                unused_bytes: 0,
                used_bytes: 0,
            }
        }
    }
}

pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let mut clip =
        arboard::Clipboard::new().map_err(|e| format!("Failed to open clipboard: {}", e))?;
    clip.set_text(text)
        .map_err(|e| format!("Failed to set clipboard text: {}", e))
}

/// Execute a command with UAC elevation using ShellExecuteExW
pub fn run_command_with_elevation(program_name: &str, args: Vec<String>) -> Result<(), String> {
    let exit_code = run_elevated_and_wait(program_name, args, false, None)?;
    if exit_code == 0 {
        Ok(())
    } else {
        Err(format!("Process exited with code {}", exit_code))
    }
}

/// Execute a command with UAC elevation using ShellExecuteExW and return its exit code.
/// `show_window`: if true, the console window is visible (for user-facing operations like install).
/// `timeout_secs`: optional timeout in seconds. None means wait forever.
pub fn run_elevated_and_wait(
    program_name: &str,
    args: Vec<String>,
    show_window: bool,
    timeout_secs: Option<u64>,
) -> Result<u32, String> {
    use tracing::debug;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
    use windows::Win32::UI::Shell::{
        SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    use windows::core::{HSTRING, PCWSTR};

    let n_show = if show_window {
        windows::Win32::UI::WindowsAndMessaging::SW_SHOW
    } else {
        SW_HIDE
    };

    let args_str = args.join(" ");
    let program = HSTRING::from(program_name);
    let parameters = HSTRING::from(&args_str);
    let verb = HSTRING::from("runas");

    debug!(
        "Executing elevated command: {} {} (show={})",
        program_name, args_str, show_window
    );

    let sys_dir = HSTRING::from("C:\\Windows\\System32");

    let mut sei = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(program.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        lpDirectory: PCWSTR(sys_dir.as_ptr()),
        nShow: n_show.0 as i32,
        ..Default::default()
    };

    let wait_ms = timeout_secs.map(|s| (s * 1000) as u32).unwrap_or(INFINITE);

    unsafe {
        match ShellExecuteExW(&mut sei) {
            Ok(()) => {
                if !sei.hProcess.is_invalid() {
                    WaitForSingleObject(sei.hProcess, wait_ms);
                    let mut exit_code: u32 = 0;
                    let _ = GetExitCodeProcess(sei.hProcess, &mut exit_code);
                    let _ = CloseHandle(sei.hProcess);
                    Ok(exit_code)
                } else {
                    Ok(0)
                }
            }
            Err(e) => Err(i18n::tr("network.error_uac_detail", &[e.to_string()])),
        }
    }
}

/// Execute a command completely invisibly with elevation.
pub fn run_invisible_elevated_commands(commands: Vec<String>) -> Result<(), String> {
    use tracing::info;

    if commands.is_empty() {
        return Ok(());
    }

    // Join commands with ' & '
    let combined = commands.join(" & ");

    info!(
        "Requesting invisible elevated execution for {} commands via cmd.exe",
        commands.len()
    );

    run_command_with_elevation(
        "cmd.exe",
        vec!["/c".to_string(), format!("\"{}\"", combined)],
    )
}

pub fn run_invisible_elevated_command(command: &str) -> Result<(), String> {
    run_invisible_elevated_commands(vec![command.to_string()])
}

/// Asynchronously clean up legacy VBS startup script (shell:startup)
pub fn cleanup_legacy_vbs_startup() {
    std::thread::spawn(|| {
        use tracing::{error, info, warn};

        // Get the current user's AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup
        let home_dir = match dirs::home_dir() {
            Some(path) => path,
            None => return,
        };

        let vbs_path = home_dir
            .join("AppData")
            .join("Roaming")
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Startup")
            .join("wsl-dashboard.vbs");

        if vbs_path.exists() {
            info!(
                "Legacy startup VBS found: {:?}. Attempting cleanup...",
                vbs_path
            );

            // Attempt to delete the file with a 3-second timeout constraint (prevents permanent blockage by antivirus)
            let path_to_del = vbs_path.clone();
            let (tx, rx) = std::sync::mpsc::channel();

            std::thread::spawn(move || {
                let res = std::fs::remove_file(&path_to_del);
                let _ = tx.send(res);
            });

            match rx.recv_timeout(std::time::Duration::from_secs(3)) {
                Ok(Ok(_)) => info!("Successfully removed legacy VBS startup script."),
                Ok(Err(e)) => error!("Failed to remove legacy VBS script: {}", e),
                Err(_) => warn!(
                    "Cleanup of legacy VBS script timed out (possible antivirus interference)."
                ),
            }
        }
    });
}
