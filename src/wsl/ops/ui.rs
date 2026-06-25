use crate::config::TerminalPreset;
use crate::utils::system::CREATE_NO_WINDOW;
use crate::wsl::executor::WslCommandExecutor;
use crate::wsl::models::WslCommandResult;
use crate::wsl::terminal;
use tokio::task;
use tracing::info;

pub async fn open_distro_folder(
    _executor: &WslCommandExecutor,
    distro_name: &str,
) -> WslCommandResult<String> {
    let path = format!(r"\\wsl$\{}", distro_name);
    task::spawn_blocking(move || {
        let mut command = std::process::Command::new("explorer.exe");
        command.arg(path);

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;

            command.creation_flags(CREATE_NO_WINDOW);
        }
        let output = command.status();
        match output {
            Ok(_) => WslCommandResult::success(String::new(), None),
            Err(e) => WslCommandResult::error(String::new(), e.to_string()),
        }
    })
    .await
    .unwrap()
}

pub async fn open_distro_vscode(
    _executor: &WslCommandExecutor,
    distro_name: &str,
    working_dir: &str,
) -> WslCommandResult<String> {
    let remote_arg = format!("wsl+{}", distro_name);
    let dir = working_dir.to_string();
    task::spawn_blocking(move || {
        let mut command = std::process::Command::new("powershell");
        // Using -Command with formatted string ensures it's executed correctly in PS
        let ps_command = format!("code --remote {} '{}'", remote_arg, dir);
        command.args(&["-NoProfile", "-Command", &ps_command]);

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;

            command.creation_flags(CREATE_NO_WINDOW);
        }
        let output = command.status();
        match output {
            Ok(_) => WslCommandResult::success(String::new(), None),
            Err(e) => WslCommandResult::error(String::new(), e.to_string()),
        }
    })
    .await
    .unwrap()
}

pub async fn check_vscode_extension(_executor: &WslCommandExecutor) -> WslCommandResult<String> {
    task::spawn_blocking(move || {
        let mut command = std::process::Command::new("powershell");
        command.args(&["-NoProfile", "-Command", "code --list-extensions"]);

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;

            command.creation_flags(CREATE_NO_WINDOW);
        }

        let output = command.output();
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                if out.status.success() {
                    WslCommandResult::success(stdout, None)
                } else {
                    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                    WslCommandResult::error(stdout, stderr)
                }
            }
            Err(e) => WslCommandResult::error(String::new(), e.to_string()),
        }
    })
    .await
    .unwrap()
}

pub async fn open_distro_terminal(
    _executor: &WslCommandExecutor,
    distro_name: &str,
    working_dir: &str,
    proxy_exports: Option<Vec<(String, String)>>,
    terminal_preset: &TerminalPreset,
) -> WslCommandResult<String> {
    let name = distro_name.to_string();
    let cd_path = working_dir.to_string();
    let preset = terminal_preset.clone();
    let proxy_exports_owned = proxy_exports.clone();

    task::spawn_blocking(move || {
        let mut command =
            terminal::build_command(&preset, &name, &cd_path, proxy_exports_owned.as_deref());
        info!(
            "Opening terminal for '{}': {} {}",
            name,
            preset.path,
            preset
                .args
                .replace("{distro}", &name)
                .replace("{dir}", &cd_path)
                .replace("{proxy}", "")
        );
        match command.spawn() {
            Ok(_) => WslCommandResult::success(String::new(), None),
            Err(e) => WslCommandResult::error(String::new(), e.to_string()),
        }
    })
    .await
    .unwrap()
}

pub async fn open_distro_folder_path(
    _executor: &WslCommandExecutor,
    distro_name: &str,
    sub_path: &str,
) -> WslCommandResult<String> {
    let path = if sub_path == "~" {
        format!(r"\\wsl$\{}", distro_name)
    } else {
        format!(r"\\wsl$\{}\{}", distro_name, sub_path.replace("/", "\\"))
    };

    task::spawn_blocking(move || {
        let mut command = std::process::Command::new("explorer.exe");
        command.arg(path);

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;

            command.creation_flags(CREATE_NO_WINDOW);
        }
        let output = command.status();
        match output {
            Ok(_) => WslCommandResult::success(String::new(), None),
            Err(e) => WslCommandResult::error(String::new(), e.to_string()),
        }
    })
    .await
    .unwrap()
}
