use crate::wsl::executor::WslCommandExecutor;
use crate::wsl::models::WslCommandResult;
use crate::wsl::models::WslVersion;

pub async fn export_distro(
    executor: &WslCommandExecutor,
    distro_name: &str,
    file_path: &str,
) -> WslCommandResult<String> {
    let mut args = vec!["--export", distro_name, file_path];
    if file_path.ends_with(".tar.gz") {
        args.extend_from_slice(&["--format", "tar.gz"]);
    }
    executor.execute_command(&args).await
}

pub async fn import_distro(
    executor: &WslCommandExecutor,
    distro_name: &str,
    install_location: &str,
    file_path: &str,
    is_vhd: bool,
) -> WslCommandResult<String> {
    let mut args = vec!["--import", distro_name, install_location, file_path];
    if is_vhd {
        args.extend_from_slice(&["--version", WslVersion::V2.as_string(), "--vhd"]);
    }
    executor.execute_command(&args).await
}
