use crate::wsl::dashboard::WslDashboard;

use tracing::error;

/// RAII Guard for managing active operations on a per-distro basis.
/// Automatically unregisters the operation when dropped.
pub struct DistroOpGuard {
    dashboard: WslDashboard,
    distro_name: String,
}

impl DistroOpGuard {
    /// Creates a new guard and registers the operation.
    pub async fn create(dashboard: WslDashboard, distro_name: String, op_name: String) -> Self {
        dashboard.register_operation(distro_name.clone(), op_name).await;
        Self {
            dashboard,
            distro_name,
        }
    }
}

impl Drop for DistroOpGuard {
    fn drop(&mut self) {
        // Since drop is synchronous, we need to spawn a task or use a blocking executor
        // to call the async unregister method.
        let dashboard = self.dashboard.clone();
        let name = self.distro_name.clone();
        
        // Spawn a background task to unregister, as we don't want to block the thread
        // or enter a runtime if one isn't available.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                dashboard.unregister_operation(&name).await;
            });
        } else {
            error!("DistroOpGuard: Failed to get tokio handle for unregistration of '{}'", self.distro_name);
        }
    }
}
