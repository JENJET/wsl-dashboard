use crate::utils::system::CREATE_NO_WINDOW;
use std::os::windows::process::CommandExt;
use std::process::Command;
use tracing::info;

/// Get the IP address of the specified distribution
/// `max_retries`: 1 for quick mode (no retry), 30 for full mode (with retry)
pub fn get_distro_ip(distro_name: &str, max_retries: Option<u32>) -> Result<String, String> {
    let max_retries = max_retries.unwrap_or(30);
    info!(
        "Fetching IP for distro: {} (max_retries: {})",
        distro_name, max_retries
    );

    let mut last_error = String::new();
    for attempt in 1..=max_retries {
        if attempt > 1 {
            info!(
                "Retrying IP fetch for {} (attempt {}/{})...",
                distro_name, attempt, max_retries
            );
            std::thread::sleep(std::time::Duration::from_secs(1));
        }

        let output = Command::new("wsl")
            .env("WSL_UTF8", "1")
            .args(&["-d", distro_name, "--", "hostname", "-I"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let stdout = crate::wsl::decoder::decode_output(&out.stdout)
                    .trim()
                    .to_string();
                if !stdout.is_empty() {
                    let ips: Vec<&str> = stdout.split_whitespace().collect();
                    info!(
                        "Found candidate IPs for {} (attempt {}): {:?}",
                        distro_name, attempt, ips
                    );

                    // Use unified IP selection logic
                    if let Some(selected_ip) = select_best_ip(&ips) {
                        info!("Selected IP: {} for {}", selected_ip, distro_name);
                        return Ok(selected_ip);
                    }
                }
            }
            Ok(out) => {
                last_error = format!(
                    "wsl command exited with error: {}",
                    crate::wsl::decoder::decode_output(&out.stderr).trim()
                );
            }
            Err(e) => {
                last_error = format!("Failed to execute wsl: {}", e);
            }
        }

        if let Some(ip) = parse_ip_from_addr(distro_name) {
            info!(
                "Found IP via ip addr fallback (attempt {}): {}",
                attempt, ip
            );
            return Ok(ip);
        } else if last_error.is_empty() {
            last_error = "hostname -I returned empty result".to_string();
        }
    }

    Err(format!(
        "Could not find IPv4 address for {} after {} attempts. Last error: {}",
        distro_name, max_retries, last_error
    ))
}

/// Select the best IP address from a list based on network mode
fn select_best_ip(ips: &[&str]) -> Option<String> {
    if ips.is_empty() {
        return None;
    }

    let is_mirrored = crate::utils::wsl_config::get_wsl_networking_mode() == "mirrored";

    if is_mirrored {
        // For mirrored mode: prefer LAN IPs (192.168.x.x or 10.x.x.x)
        if let Some(lan_ip) = ips
            .iter()
            .find(|&&ip| ip.starts_with("192.168.") || ip.starts_with("10."))
        {
            return Some(lan_ip.to_string());
        }
    }

    // Fallback: prefer WSL bridge IP (172.x.x.x)
    if let Some(wsl_ip) = ips.iter().find(|&&ip| ip.starts_with("172.")) {
        return Some(wsl_ip.to_string());
    }

    // Last resort: use first available IP
    ips.first().map(|ip| ip.to_string())
}

fn parse_ip_from_addr(distro_name: &str) -> Option<String> {
    let output = Command::new("wsl")
        .env("WSL_UTF8", "1")
        .args(&["-d", distro_name, "--", "ip", "-4", "addr", "show"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            let stdout = crate::wsl::decoder::decode_output(&out.stdout);
            let mut is_lo = false;
            let mut ips: Vec<&str> = Vec::new();

            for line in stdout.lines() {
                let line = line.trim();
                if line.starts_with(|c: char| c.is_ascii_digit()) {
                    is_lo = line.contains(": lo:");
                }
                if is_lo {
                    continue;
                }
                if line.starts_with("inet ") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() > 1 {
                        let ip_cidr = parts[1];
                        let ip = ip_cidr.split('/').next().unwrap_or(ip_cidr);
                        if ip != "127.0.0.1" {
                            ips.push(ip);
                        }
                    }
                }
            }

            // Use the same selection logic as hostname -I
            return select_best_ip(&ips);
        }
    }
    None
}

/// Check if the distribution is currently running (fast check, won't start it)
pub fn is_distro_running(distro_name: &str) -> bool {
    let output = Command::new("wsl")
        .env("WSL_UTF8", "1")
        .args(&["-l", "-q", "--running"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            let stdout = crate::wsl::decoder::decode_output(&out.stdout);
            return stdout
                .lines()
                .any(|l| l.trim().eq_ignore_ascii_case(distro_name));
        }
    }
    false
}

/// Get CPU and memory usage for a running WSL distro
/// Returns (cpu_percentage, used_memory_gb, total_memory_gb)
pub fn get_distro_resource_usage(distro_name: &str) -> Result<(f64, f64, f64), String> {
    // Check if distro is actually running before attempting to fetch resources
    // This prevents waking up stopped distros
    if !is_distro_running(distro_name) {
        return Err(format!("Distro '{}' is not running", distro_name));
    }

    // Get memory info from /proc/meminfo
    let mem_output = Command::new("wsl")
        .env("WSL_UTF8", "1")
        .args(&[
            "-d",
            distro_name,
            "--",
            "sh",
            "-c",
            "grep -E '^(MemTotal|MemAvailable):' /proc/meminfo | sed 's/[^0-9]//g'",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    let (mem_total_kb, mem_available_kb) = match mem_output {
        Ok(out) if out.status.success() => {
            let stdout = crate::wsl::decoder::decode_output(&out.stdout);
            tracing::debug!(
                "Memory info raw output for {}: {}",
                distro_name,
                stdout.trim()
            );
            let lines: Vec<&str> = stdout.lines().collect();
            if lines.len() >= 2 {
                let total = lines[0].trim().parse::<u64>().unwrap_or(0);
                let available = lines[1].trim().parse::<u64>().unwrap_or(0);
                tracing::debug!(
                    "Memory for {}: total={} KB, available={} KB",
                    distro_name,
                    total,
                    available
                );
                if total == 0 {
                    return Err("Memory total is 0".to_string());
                }
                (total, available)
            } else {
                return Err(format!(
                    "Failed to parse memory info, got {} lines",
                    lines.len()
                ));
            }
        }
        Ok(out) => {
            let stderr = crate::wsl::decoder::decode_output(&out.stderr);
            return Err(format!("Memory command failed: {}", stderr.trim()));
        }
        Err(e) => return Err(format!("Failed to execute memory command: {}", e)),
    };

    let mem_used_kb = mem_total_kb.saturating_sub(mem_available_kb);

    // Convert to GB with one decimal place
    let mem_total_gb = mem_total_kb as f64 / (1024.0 * 1024.0);
    let mem_used_gb = mem_used_kb as f64 / (1024.0 * 1024.0);

    tracing::debug!(
        "Memory calculation for {}: total={} GB, used={} GB, available={} MB",
        distro_name,
        mem_total_gb,
        mem_used_gb,
        mem_available_kb / 1024
    );

    // Get CPU usage using /proc/stat
    // Read two samples 100ms apart to calculate CPU usage
    let cpu_percent = match get_cpu_usage(distro_name) {
        Ok(cpu) => {
            tracing::debug!("CPU usage for {}: {:.2}%", distro_name, cpu);
            cpu
        }
        Err(e) => {
            tracing::warn!("Failed to get CPU usage for {}: {}", distro_name, e);
            0.0
        }
    };

    tracing::info!(
        "Resource usage for {}: CPU={:.1}%, Memory={:.1}/{:.1} GB",
        distro_name,
        cpu_percent,
        mem_used_gb,
        mem_total_gb
    );

    Ok((cpu_percent, mem_used_gb, mem_total_gb))
}

/// Calculate CPU usage by reading /proc/stat twice with a small delay
fn get_cpu_usage(distro_name: &str) -> Result<f64, String> {
    // First sample
    let stat1 = read_proc_stat(distro_name)?;

    // Wait 100ms
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Second sample
    let stat2 = read_proc_stat(distro_name)?;

    // Calculate CPU usage percentage
    let user_diff = stat2.user.saturating_sub(stat1.user);
    let nice_diff = stat2.nice.saturating_sub(stat1.nice);
    let system_diff = stat2.system.saturating_sub(stat1.system);
    let idle_diff = stat2.idle.saturating_sub(stat1.idle);
    let iowait_diff = stat2
        .iowait
        .unwrap_or(0)
        .saturating_sub(stat1.iowait.unwrap_or(0));

    let total_diff = user_diff + nice_diff + system_diff + idle_diff + iowait_diff;
    let active_diff = user_diff + nice_diff + system_diff + iowait_diff;

    if total_diff == 0 {
        return Ok(0.0);
    }

    let cpu_percent = (active_diff as f64 / total_diff as f64) * 100.0;
    Ok(cpu_percent)
}

#[derive(Debug)]
struct CpuStat {
    user: u64,
    nice: u64,
    system: u64,
    idle: u64,
    iowait: Option<u64>,
}

/// Read /proc/stat and parse CPU line
fn read_proc_stat(distro_name: &str) -> Result<CpuStat, String> {
    let output = Command::new("wsl")
        .env("WSL_UTF8", "1")
        .args(&["-d", distro_name, "--", "sh", "-c", "head -1 /proc/stat"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = crate::wsl::decoder::decode_output(&out.stdout);
            tracing::debug!(
                "/proc/stat raw output for {}: {}",
                distro_name,
                stdout.trim()
            );
            // Format: cpu  user nice system idle iowait irq softirq steal guest guest_nice
            let parts: Vec<&str> = stdout.split_whitespace().collect();
            if parts.len() < 5 || parts[0] != "cpu" {
                return Err(format!("Invalid /proc/stat format: {}", stdout.trim()));
            }

            let user = parts[1].parse::<u64>().unwrap_or(0);
            let nice = parts[2].parse::<u64>().unwrap_or(0);
            let system = parts[3].parse::<u64>().unwrap_or(0);
            let idle = parts[4].parse::<u64>().unwrap_or(0);
            let iowait = if parts.len() > 5 {
                parts[5].parse::<u64>().ok()
            } else {
                None
            };

            tracing::debug!(
                "CPU stat for {}: user={}, nice={}, system={}, idle={}, iowait={:?}",
                distro_name,
                user,
                nice,
                system,
                idle,
                iowait
            );

            Ok(CpuStat {
                user,
                nice,
                system,
                idle,
                iowait,
            })
        }
        Ok(out) => {
            let stderr = crate::wsl::decoder::decode_output(&out.stderr);
            Err(format!("Failed to read /proc/stat: {}", stderr.trim()))
        }
        Err(e) => Err(format!("Failed to execute command: {}", e)),
    }
}
