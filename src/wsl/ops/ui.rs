use crate::utils::system::CREATE_NO_WINDOW;
use crate::wsl::executor::WslCommandExecutor;
use crate::wsl::models::WslCommandResult;
use tokio::task;

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
) -> WslCommandResult<String> {
    let name = distro_name.to_string();
    let cd_path = working_dir.to_string();
    let proxy_exports = proxy_exports.clone();

    task::spawn_blocking(move || {
        let mut command = std::process::Command::new("cmd");
        let mut cmd_str = String::new();

        if let Some(exports) = proxy_exports {
            let mut wslenv = String::new();

            for (k, v) in exports {
                cmd_str.push_str(&format!("echo export {}={}& ", k, v));
                cmd_str.push_str(&format!("set \"{}={}\"& ", k, v));
                wslenv.push_str(&format!("{}/u:", k));
            }
            if !wslenv.is_empty() {
                wslenv.pop(); // remove trailing colon
                cmd_str.push_str(&format!("set \"WSLENV={}\"& ", wslenv));
            }
        }

        cmd_str.push_str(&format!("wsl -d {} --cd {}", name, cd_path));
        command.args(&[
            "/c",
            "start",
            &format!("WSL: {}", name),
            "cmd",
            "/c",
            &cmd_str,
        ]);

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
