use serde::{Deserialize, Serialize};

// WSL subsystem version
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WslVersion {
    V1,
    V2,
}

// WSL subsystem status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WslStatus {
    Running,
    Stopped,
}

// WSL subsystem information
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WslDistro {
    pub name: String,
    pub status: WslStatus,
    pub version: WslVersion,
    pub is_default: bool,
    pub last_start_time: Option<std::time::SystemTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WslInformation {
    pub distro_name: String,
    pub wsl_version: String,
    pub status: String,
    pub install_location: String,
    pub vhdx_path: String,
    pub vhdx_size: String,
    pub actual_used: String,
    pub ip: String,
    pub package_family_name: String,
    pub vhdx_virtual_size: String,
    pub vhdx_type: String,
    pub vhdx_is_sparse: bool,
    pub actual_unused: String,
}

impl WslDistro {
    // Check if two WSL subsystems are logically equal (ignore startup time)
    pub fn business_equals(&self, other: &WslDistro) -> bool {
        self.name == other.name
            && self.status == other.status
            && self.version == other.version
            && self.is_default == other.is_default
    }
}

// WSL command execution result
#[derive(Debug, Clone)]
pub struct WslCommandResult<T> {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    pub data: Option<T>,
    pub timeout: bool,
}

impl<T> WslCommandResult<T> {
    #[allow(dead_code)]
    pub fn new(success: bool, output: String, error: Option<String>, data: Option<T>) -> Self {
        Self {
            success,
            output,
            error,
            data,
            timeout: false,
        }
    }

    pub fn success(output: String, data: Option<T>) -> Self {
        Self {
            success: true,
            output,
            error: None,
            data,
            timeout: false,
        }
    }

    pub fn error(output: String, error: String) -> Self {
        Self {
            success: false,
            output,
            error: Some(error),
            data: None,
            timeout: false,
        }
    }
}
