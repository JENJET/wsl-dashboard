use crate::config::ConfigManager;
use crate::utils::logging::LoggingSystem;
use crate::wsl::dashboard::WslDashboard;
use crate::wsl::models::{MountedDisk, WslDistro, WslStatus, WslVersion};
use std::collections::HashSet;

// Define application state
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct VSCodeExtensionData {
    pub name: String,
    pub url: String,
}

pub struct AppState {
    pub wsl_dashboard: WslDashboard,
    pub config_manager: ConfigManager,
    pub logging_system: Option<LoggingSystem>,
    pub vscode_extension: Option<VSCodeExtensionData>,
    pub is_silent_mode: bool,
    pub theme_watcher: Option<crate::utils::theme::ThemeWatcher>,
    pub selected_distros: HashSet<String>,
    pub mounted_disks: Vec<MountedDisk>,
}

impl AppState {
    pub fn new(
        config_manager: ConfigManager,
        logging_system: LoggingSystem,
        is_silent_mode: bool,
    ) -> Self {
        // Load initial distros from cache for fast startup (warm start)
        let cached = config_manager.get_cached_distros();
        let initial_distros: Vec<WslDistro> = cached
            .into_iter()
            .map(|c| WslDistro {
                name: c.name,
                status: match c.status.as_str() {
                    "Running" => WslStatus::Running,
                    "Stopped" => WslStatus::Stopped,
                    "Installing" => WslStatus::Installing,
                    "Converting" => WslStatus::Converting,
                    "Uninstalling" => WslStatus::Uninstalling,
                    "Exporting" => WslStatus::Exporting,
                    "Deleting" => WslStatus::Deleting,
                    "Disabled" => WslStatus::Disabled,
                    other => WslStatus::Unknown(other.to_string()),
                },
                version: if c.version == "V1" || c.version == "1" {
                    WslVersion::V1
                } else {
                    WslVersion::V2
                },
                is_default: c.is_default,
                last_start_time: None,
            })
            .collect();

        Self {
            wsl_dashboard: WslDashboard::new(initial_distros),
            config_manager,
            logging_system: Some(logging_system),
            vscode_extension: None,
            is_silent_mode,
            theme_watcher: None,
            selected_distros: HashSet::new(),
            mounted_disks: Vec::new(),
        }
    }
}
