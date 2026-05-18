use crate::wsl::dashboard::WslDashboard;

/// RAII Guard for managing active operations on a per-distro basis.
/// Automatically unregisters the operation when dropped.
/// Both register and unregister are synchronous (backed by `std::sync::Mutex`),
/// so Drop always succeeds without needing a tokio runtime handle.
pub struct DistroOpGuard {
    dashboard: WslDashboard,
    distro_name: String,
}

impl DistroOpGuard {
    /// Creates a new guard and registers the operation.
    pub async fn create(dashboard: WslDashboard, distro_name: String, op_name: String) -> Self {
        dashboard.register_operation(distro_name.clone(), op_name);
        Self {
            dashboard,
            distro_name,
        }
    }
}

impl Drop for DistroOpGuard {
    fn drop(&mut self) {
        self.dashboard.unregister_operation(&self.distro_name);
    }
}
