use semver::Version;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfficialGroupItem {
    #[serde(rename = "system-language", default)]
    pub system_language: String,
    #[serde(default)]
    pub timezone: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub pic: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StaticConfigResponse {
    pub latest: String,
    #[serde(rename = "official-group", default)]
    pub official_group: Vec<OfficialGroupItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateResult {
    pub has_update: bool,
    pub latest_version: String,
    pub current_version: String,
    pub download_url: String,
    pub error: Option<String>,
}

/// Get the complete BASE_API response (including official-group), retrying 3 times with a 5s timeout
pub async fn fetch_base_config(timezone: &str) -> Result<StaticConfigResponse, String> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let base_url = if timezone == crate::app::ZH_TIMEZONE {
        crate::app::STATIC_API
    } else {
        crate::app::STATIC_API_FREE
    };
    let url = format!("{}{}?t={}", base_url, crate::app::BASE_API, ts);

    debug!("fetch_base_config url: {}", url);

    tokio::task::spawn_blocking(move || {
        let mut last_err = None;
        let mut response_body: Option<StaticConfigResponse> = None;

        for i in 0..3 {
            if i > 0 {
                std::thread::sleep(Duration::from_secs(2));
            }
            match ureq::get(&url).timeout(Duration::from_secs(5)).call() {
                Ok(resp) => match resp.into_json::<StaticConfigResponse>() {
                    Ok(config) => {
                        response_body = Some(config);
                        break;
                    }
                    Err(e) => {
                        last_err = Some(format!("Failed to parse JSON: {}", e));
                    }
                },
                Err(e) => {
                    let e_str = e.to_string();
                    let e_lower = e_str.to_lowercase();
                    if e_lower.contains("timed out")
                        || e_lower.contains("timeout")
                        || e_lower.contains("10060")
                    {
                        last_err = Some("RequestTimeOut".to_string());
                    } else {
                        last_err = Some(format!("Request error: {}", e));
                    }
                }
            }
        }

        response_body.ok_or_else(|| last_err.unwrap_or_else(|| "Unknown error".to_string()))
    })
    .await
    .map_err(|e| format!("Task panicked: {}", e))?
}

pub async fn check_update(
    current_version_str: &str,
    timezone: &str,
) -> Result<UpdateResult, String> {
    let current_version_str = current_version_str.to_string();
    let timezone_str = timezone.to_string();

    let config = fetch_base_config(timezone).await?;

    // Version comparison
    let current_v_clean = current_version_str.trim_start_matches('v');
    let latest_v_clean = config.latest.trim_start_matches('v');

    let current = Version::parse(current_v_clean).map_err(|e| {
        format!(
            "Failed to parse current version {}: {}",
            current_version_str, e
        )
    })?;
    let latest = Version::parse(latest_v_clean)
        .map_err(|e| format!("Failed to parse latest version {}: {}", config.latest, e))?;

    info!("update check_update,latest: {}", config.latest);

    let base_github_url = if timezone_str == crate::app::ZH_TIMEZONE {
        crate::app::GITEE_URL
    } else {
        crate::app::GITHUB_URL
    };
    let download_url = format!("{}{}", base_github_url, crate::app::GITHUB_RELEASES);

    Ok(UpdateResult {
        has_update: latest > current,
        latest_version: config.latest,
        current_version: current_version_str,
        download_url,
        error: None,
    })
}
