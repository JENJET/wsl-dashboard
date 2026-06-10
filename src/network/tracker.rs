use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::info;

static NUM_CORES_CACHE: Lazy<Mutex<HashMap<String, usize>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Global flag to skip resource/IP fetching during batch operations.
/// Set by batch handlers alongside app.set_batch_operating().
pub static BATCH_OPERATING: AtomicBool = AtomicBool::new(false);

/// Get the IP address of the specified distribution
/// `max_retries`: 1 for quick mode (no retry), 30 for full mode (with retry)
pub fn get_distro_ip(distro_name: &str, max_retries: Option<u32>) -> Result<String, String> {
    if BATCH_OPERATING.load(Ordering::Relaxed) {
        return Err("Batch operation in progress".to_string());
    }
    if !is_distro_running(distro_name) {
        return Err(format!("Distro '{}' is not running", distro_name));
    }
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

        let output = crate::utils::system::new_wsl_command()
            .args(["-d", distro_name, "--", "hostname", "-I"])
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
    let output = crate::utils::system::new_wsl_command()
        .args(["-d", distro_name, "--", "ip", "-4", "addr", "show"])
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
    let output = crate::utils::system::new_wsl_command()
        .args(["-l", "-q", "--running"])
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

/// Get CPU & memory usage for a running WSL distro
pub fn get_distro_resource_usage(distro_name: &str) -> (f64, f64) {
    if BATCH_OPERATING.load(Ordering::Relaxed) {
        return (0.0, 0.0);
    }
    if !is_distro_running(distro_name) {
        return (0.0, 0.0);
    }
    get_cpu_and_mem(distro_name).unwrap_or_else(|e| {
        tracing::warn!("Failed to get resource usage for {}: {}", distro_name, e);
        (0.0, 0.0)
    })
}

/// Get the number of CPU cores from inside the WSL distro
fn get_distro_num_cores(distro_name: &str) -> usize {
    if let Some(cached) = NUM_CORES_CACHE
        .lock()
        .ok()
        .and_then(|m| m.get(distro_name).copied())
    {
        return cached;
    }

    let count = crate::utils::system::new_wsl_command()
        .args(["-d", distro_name, "--", "nproc"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let s = crate::wsl::decoder::decode_output(&o.stdout);
                s.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(1)
        .max(1);

    if let Ok(mut cache) = NUM_CORES_CACHE.lock() {
        cache.insert(distro_name.to_string(), count);
    }
    count
}

/// Calculate CPU & memory usage using top -bn2, with ps fallback for minimal distros
/// (e.g. docker-desktop) that may not have top installed.
fn get_cpu_and_mem(distro_name: &str) -> Result<(f64, f64), String> {
    let result = get_cpu_and_mem_via_top(distro_name);
    if result.is_ok() {
        return result;
    }
    tracing::debug!(
        "[{}] top failed, falling back to ps: {}",
        distro_name,
        result.unwrap_err()
    );
    get_cpu_and_mem_via_ps(distro_name)
}

fn get_cpu_and_mem_via_top(distro_name: &str) -> Result<(f64, f64), String> {
    let output = crate::utils::system::new_wsl_command()
        .args(["-d", distro_name, "--", "top", "-bn2", "-d", "0.2", "-b"])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = crate::wsl::decoder::decode_output(&out.stdout);
            let trimmed = stdout.trim();

            // Detect BusyBox vs GNU top format.
            // GNU: "%Cpu(s): ..."   BusyBox: "CPU: ..."
            let is_busybox = trimmed.lines().any(|l| l.starts_with("CPU:"));

            let mut cpu_sum = 0.0f64;
            let mut mem_sum_kib = 0.0f64;
            let mut iteration = 0u32;

            for line in trimmed.lines() {
                // Count iteration boundaries
                if line.contains("%Cpu") || line.starts_with("CPU:") {
                    iteration += 1;
                    continue;
                }

                if iteration < 2 {
                    continue;
                }

                let fields: Vec<&str> = line.split_whitespace().collect();
                if fields.is_empty() || !fields[0].chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }

                if is_busybox {
                    // BusyBox: PID PPID USER STAT VSZ %VSZ CPU %CPU COMMAND
                    //          [0]  [1]   [2]  [3]  [4] [5]  [6] [7]  [8]
                    if fields.len() >= 8 {
                        let cpu_str = fields[7].trim_end_matches('%');
                        if let Ok(cpu) = cpu_str.parse::<f64>() {
                            cpu_sum += cpu;
                        }
                    }
                } else {
                    // GNU: PID USER PR NI VIRT RES SHR S %CPU %MEM TIME+ COMMAND
                    //       [0]  [1]  [2][3] [4] [5] [6][7] [8]  [9] [10]  [11]
                    if fields.len() >= 10 {
                        if let Ok(cpu) = fields[8].parse::<f64>() {
                            cpu_sum += cpu;
                        }
                        if let Ok(res) = fields[5].parse::<f64>() {
                            mem_sum_kib += res;
                        }
                    }
                }
            }

            // BusyBox: sum VmRSS from /proc (same as GNU top RES, no top dependency)
            if is_busybox {
                mem_sum_kib = get_busybox_proc_mem(distro_name).unwrap_or(0.0);
            }

            let num_cores = get_distro_num_cores(distro_name);
            let cpu_percent = (cpu_sum / num_cores as f64).min(100.0);
            tracing::debug!(
                "[{}] top(busybox={}): cpu={} mem_kib={}",
                distro_name,
                is_busybox,
                cpu_percent,
                mem_sum_kib
            );
            Ok((cpu_percent, mem_sum_kib))
        }
        Ok(out) => {
            let stderr = crate::wsl::decoder::decode_output(&out.stderr);
            Err(format!("top command failed: {}", stderr.trim()))
        }
        Err(e) => Err(format!("Failed to execute top command: {}", e)),
    }
}

/// Read memory from /proc/*/status VmRSS for BusyBox distros.
/// Sums all processes' VmRSS (same as GNU top RES), returns KiB.
fn get_busybox_proc_mem(distro_name: &str) -> Option<f64> {
    let output = crate::utils::system::new_wsl_command()
        .args([
            "-d",
            distro_name,
            "--",
            "grep",
            "-h",
            "VmRSS:",
            "/proc/[0-9]*/status",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = crate::wsl::decoder::decode_output(&output.stdout);
    let mut total_kib = 0.0f64;
    for line in stdout.lines() {
        // Format: "VmRSS:    875020 kB"
        if let Some(val_part) = line.split_whitespace().nth(1) {
            if let Ok(kib) = val_part.parse::<f64>() {
                total_kib += kib;
            }
        }
    }
    Some(total_kib)
}

fn get_cpu_and_mem_via_ps(distro_name: &str) -> Result<(f64, f64), String> {
    let output = crate::utils::system::new_wsl_command()
        .args(["-d", distro_name, "--", "ps", "-eo", "pid,%cpu,rss"])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = crate::wsl::decoder::decode_output(&out.stdout);
            let trimmed = stdout.trim();

            let mut cpu_sum = 0.0f64;
            let mut mem_sum = 0.0f64;
            let mut header_skipped = false;

            for line in trimmed.lines() {
                if !header_skipped {
                    header_skipped = true;
                    continue;
                }
                let fields: Vec<&str> = line.split_whitespace().collect();
                // GNU ps: PID %CPU RSS
                if fields.len() >= 3 {
                    if let Ok(cpu) = fields[1].parse::<f64>() {
                        cpu_sum += cpu;
                    }
                    if let Ok(rss) = fields[2].parse::<f64>() {
                        mem_sum += rss;
                    }
                }
            }

            let num_cores = get_distro_num_cores(distro_name);
            let cpu_percent = (cpu_sum / num_cores as f64).min(100.0);
            tracing::debug!(
                "[{}] ps: cpu={} mem_kib={}",
                distro_name,
                cpu_percent,
                mem_sum
            );
            Ok((cpu_percent, mem_sum))
        }
        Ok(out) => {
            let stderr = crate::wsl::decoder::decode_output(&out.stderr);
            Err(format!("ps command failed: {}", stderr.trim()))
        }
        Err(e) => Err(format!("Failed to execute ps command: {}", e)),
    }
}
