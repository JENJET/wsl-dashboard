use ini::Ini;
use tracing::{debug, warn};

/// Get the WSL networking mode from ~/.wslconfig
/// Returns "nat" as default if file or setting is missing.
pub fn get_wsl_networking_mode() -> String {
    get_wsl_networking_mode_impl()
}

fn get_wsl_networking_mode_impl() -> String {
    let home_dir = match dirs::home_dir() {
        Some(path) => path,
        None => {
            warn!("Could not determine home directory, defaulting networkingMode to 'nat'");
            return "nat".to_string();
        }
    };

    let wsl_config_path = home_dir.join(".wslconfig");
    if !wsl_config_path.exists() {
        debug!(
            ".wslconfig not found at {:?}, defaulting networkingMode to 'nat'",
            wsl_config_path
        );
        return "nat".to_string();
    }

    match Ini::load_from_file(&wsl_config_path) {
        Ok(ini) => {
            if let Some(section) = ini.section(Some("wsl2")) {
                if let Some(mode) = section.get("networkingMode") {
                    let mode_lower = mode.to_lowercase();
                    debug!("Detected networkingMode from .wslconfig: {}", mode_lower);
                    return mode_lower;
                }
            }
            debug!("networkingMode not found in [wsl2] section of .wslconfig, defaulting to 'nat'");
            "nat".to_string()
        }
        Err(e) => {
            warn!(
                "Failed to parse .wslconfig at {:?}: {}, defaulting networkingMode to 'nat'",
                wsl_config_path, e
            );
            "nat".to_string()
        }
    }
}

/// Whether to show distro IP in the instance list.
/// Controlled by networking mode — only enabled under "virtioproxy" mode.
pub fn show_distro_ip() -> bool {
    //get_wsl_networking_mode_impl() == "virtioproxy"
    false
}
