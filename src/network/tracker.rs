use std::os::windows::process::CommandExt;
use std::process::Command;
use tracing::info;

const CREATE_NO_WINDOW: u32 = 0x08000000;

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

                    if let Some(wsl_ip) = ips.iter().find(|&&ip| ip.starts_with("172.")) {
                        info!(
                            "Selected WSL default bridge IP: {} for {}",
                            wsl_ip, distro_name
                        );
                        return Ok(wsl_ip.to_string());
                    }

                    if let Some(first_ip) = ips.first() {
                        info!(
                            "No 172.x IP found, selected first available: {} for {}",
                            first_ip, distro_name
                        );
                        return Ok(first_ip.to_string());
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
                            return Some(ip.to_string());
                        }
                    }
                }
            }
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
