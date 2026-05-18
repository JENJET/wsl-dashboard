use super::WslDashboard;
use super::operation_guard::DistroOpGuard;
use crate::wsl::models::{WslCommandResult, WslStatus};
use tokio::time::{Duration, Instant};
use tracing::{debug, info, warn};

impl WslDashboard {
    pub async fn start_distro(&self, name: &str) -> WslCommandResult<String> {
        let _guard = DistroOpGuard::create(
            self.clone(),
            name.to_string(),
            "operation.starting".to_string(),
        )
        .await;
        self.increment_manual_operation();
        let result = self.executor.start_distro(name).await;
        if result.success {
            info!(
                "WSL distro '{}' startup command executed, waiting for status update",
                name
            );
            let _ = self.refresh_distros().await;

            let manager_clone = self.clone();
            let name_clone = name.to_string();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(3)).await;
                info!(
                    "Delayed refresh of WSL distro '{}' status after startup",
                    name_clone
                );
                let _ = manager_clone.refresh_distros().await;
                manager_clone.decrement_manual_operation();
            });
        } else {
            self.decrement_manual_operation();
        }
        result
    }

    pub async fn stop_distro(&self, name: &str) -> WslCommandResult<String> {
        let _guard = DistroOpGuard::create(
            self.clone(),
            name.to_string(),
            "operation.stopping".to_string(),
        )
        .await;
        self.increment_manual_operation();
        info!("Calling executor.stop_distro for '{}'", name);
        let result = self.executor.stop_distro(name).await;
        info!(
            "Executor returned from stop_distro for '{}' (success: {})",
            name, result.success
        );

        if result.success {
            info!(
                "WSL distro '{}' termination command executed, waiting for status update",
                name
            );
            let _ = self.refresh_distros().await;
            info!("Immediate refresh after stop completed for '{}'", name);

            // Retry terminate if the distro is still running (resource monitor may have delayed VM shutdown)
            let max_retries = 3;
            for attempt in 1..=max_retries {
                if let Some(distro) = self.get_distro(name).await {
                    if matches!(distro.status, WslStatus::Stopped) {
                        break;
                    }
                }
                info!(
                    "Distro '{}' still running after terminate, retrying (attempt {}/{})",
                    name, attempt, max_retries
                );
                let _ = self.executor.stop_distro(name).await;
                let _ = self.refresh_distros().await;
                tokio::time::sleep(Duration::from_millis(500)).await;
            }

            let manager_clone = self.clone();
            let name_clone = name.to_string();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(3)).await;
                info!(
                    "Delayed refresh of WSL distro '{}' status after termination",
                    name_clone
                );
                let _ = manager_clone.refresh_distros().await;
                manager_clone.decrement_manual_operation();
            });
        } else {
            self.decrement_manual_operation();
        }
        result
    }

    pub async fn restart_distro(&self, name: &str) -> WslCommandResult<String> {
        let _guard = DistroOpGuard::create(
            self.clone(),
            name.to_string(),
            "operation.restarting".to_string(),
        )
        .await;
        self.increment_manual_operation();
        info!("WSL distro '{}' restart initiated", name);

        // 1. Terminate via executor directly (not self.stop_distro) to avoid
        // nested increment/decrement and duplicate DistroOpGuard.
        info!("Stopping '{}' as part of restart...", name);
        let stop_result = self.executor.stop_distro(name).await;
        if !stop_result.success {
            warn!(
                "Stop failed during restart for '{}', aborting restart",
                name
            );
            self.decrement_manual_operation();
            return stop_result;
        }

        // 2. Refresh and poll for Stopped status (Smart Wait)
        info!(
            "Stop successful for '{}', polling for Stopped status...",
            name
        );
        let _ = self.refresh_distros().await;

        let start_wait = Instant::now();
        let timeout = Duration::from_secs(10);
        let mut is_stopped = false;

        while start_wait.elapsed() < timeout {
            let _ = self.refresh_distros().await;
            if let Some(distro) = self.get_distro(name).await {
                if matches!(distro.status, WslStatus::Stopped) {
                    debug!(
                        "Distro '{}' confirmed Stopped after {}ms",
                        name,
                        start_wait.elapsed().as_millis()
                    );
                    is_stopped = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        if !is_stopped {
            warn!(
                "Distro '{}' did not reach Stopped status within timeout, forcing start attempt anyway",
                name
            );
        } else {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // 3. Start via executor directly
        info!("Initiating start for '{}'...", name);
        let start_result = self.executor.start_distro(name).await;

        if start_result.success {
            info!(
                "WSL distro '{}' startup command executed (restart), waiting for status update",
                name
            );
            let _ = self.refresh_distros().await;

            let manager_clone = self.clone();
            let name_clone = name.to_string();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(3)).await;
                info!(
                    "Delayed refresh of WSL distro '{}' status after restart",
                    name_clone
                );
                let _ = manager_clone.refresh_distros().await;
                manager_clone.decrement_manual_operation();
            });
        } else {
            self.decrement_manual_operation();
        }

        start_result
    }

    pub async fn shutdown_wsl(&self) -> WslCommandResult<String> {
        let _heavy_lock = self.heavy_op_lock.lock().await;
        self.increment_manual_operation();
        info!("Initiating WSL system shutdown");
        let result = self.executor.shutdown_wsl().await;
        if result.success {
            let _ = self.refresh_distros().await;
        }
        self.decrement_manual_operation();
        result
    }

    pub async fn delete_distro(
        &self,
        config_manager: &crate::config::ConfigManager,
        name: &str,
    ) -> WslCommandResult<String> {
        let _guard = DistroOpGuard::create(
            self.clone(),
            name.to_string(),
            "operation.deleting".to_string(),
        )
        .await;
        let _heavy_lock = self.heavy_op_lock.lock().await;
        self.increment_manual_operation();

        let self_clone = self.clone();
        let _op_guard = scopeguard::guard((), |_| {
            self_clone.decrement_manual_operation();
        });

        info!(
            "Initiating deletion of WSL distro '{}' (irreversible operation)",
            name
        );
        let result = self.executor.delete_distro(config_manager, name).await;

        if result.success {
            // Immediate local update to make UI responsive
            {
                let mut distros = self.distros.lock().await;
                let old_len = distros.len();
                distros.retain(|d| d.name != name);
                if distros.len() < old_len {
                    debug!("Manually removed '{}' from local cache, notifying UI", name);
                    self.state_changed.notify_one();
                }
            }
            // Full refresh is now deferred to the background monitor once manual_operation drops to 0
        }

        // Lock is released here at end of scope
        result
    }

    pub async fn export_distro(&self, name: &str, file_path: &str) -> WslCommandResult<String> {
        let _guard = DistroOpGuard::create(
            self.clone(),
            name.to_string(),
            "operation.exporting".to_string(),
        )
        .await;
        let _heavy_lock = self.heavy_op_lock.lock().await;
        self.increment_manual_operation();
        let result = self.executor.export_distro(name, file_path).await;
        self.decrement_manual_operation();
        result
    }

    pub async fn import_distro(
        &self,
        name: &str,
        install_location: &str,
        file_path: &str,
        is_vhd: bool,
    ) -> WslCommandResult<String> {
        let _guard = DistroOpGuard::create(
            self.clone(),
            name.to_string(),
            "install.importing".to_string(),
        )
        .await;
        let _heavy_lock = self.heavy_op_lock.lock().await;
        self.increment_manual_operation();
        let result = self
            .executor
            .import_distro(name, install_location, file_path, is_vhd)
            .await;
        if result.success {
            let _ = self.refresh_distros().await;
        }
        self.decrement_manual_operation();
        result
    }

    pub async fn move_distro(
        &self,
        name: &str,
        new_path: &str,
        use_elevation: bool,
    ) -> WslCommandResult<String> {
        let _guard = DistroOpGuard::create(
            self.clone(),
            name.to_string(),
            "operation.moving".to_string(),
        )
        .await;
        let _heavy_lock = self.heavy_op_lock.lock().await;
        self.increment_manual_operation();
        let result = self
            .executor
            .move_distro(name, new_path, use_elevation)
            .await;
        if result.success {
            let _ = self.refresh_distros().await;
        }
        self.decrement_manual_operation();
        result
    }

    pub async fn open_distro_bashrc(&self, name: &str) -> WslCommandResult<String> {
        self.executor.open_distro_folder_path(name, "~").await
    }

    #[allow(dead_code)]
    pub async fn open_distro_folder(&self, distro_name: &str) -> WslCommandResult<String> {
        self.executor.open_distro_folder(distro_name).await
    }
}
