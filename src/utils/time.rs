use chrono;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::info;
use ureq;

struct WebSite {
    name: &'static str,
    url: &'static str,
}

const WEB_URLS: &[WebSite] = &[
    WebSite {
        name: "163",
        url: "https://www.163.com",
    },
    WebSite {
        name: "qq",
        url: "https://www.qq.com",
    },
    WebSite {
        name: "baidu",
        url: "https://www.baidu.com",
    },
    WebSite {
        name: "zhihu",
        url: "https://www.zhihu.com",
    },
    WebSite {
        name: "2345",
        url: "https://www.2345.com",
    },
    WebSite {
        name: "douban",
        url: "https://www.douban.com",
    },
    WebSite {
        name: "ntsc",
        url: "https://www.ntsc.ac.cn",
    },
    WebSite {
        name: "360",
        url: "https://www.360.cn",
    },
    WebSite {
        name: "beijing-time",
        url: "https://www.beijing-time.org",
    },
];

const WEB_URLS_FREE: &[WebSite] = &[
    WebSite {
        name: "bing",
        url: "https://www.bing.com",
    },
    WebSite {
        name: "google",
        url: "https://www.google.com",
    },
    WebSite {
        name: "amazon",
        url: "https://www.amazon.com",
    },
    WebSite {
        name: "facebook",
        url: "https://www.facebook.com",
    },
    WebSite {
        name: "tiktok",
        url: "https://www.tiktok.com",
    },
];

pub fn standard_time(timezone: &str) -> i64 {
    // Select URL list based on timezone
    let urls = if timezone == crate::app::ZH_TIMEZONE {
        WEB_URLS
    } else {
        WEB_URLS_FREE
    };

    // Try servers in order (reliable ones first)
    for i in 0..5.min(urls.len()) {
        let site = &urls[i];
        if let Ok(ts) = get_website_timestamp(site.url) {
            info!("Successfully obtained time from [{}]: {}", site.name, ts);
            return ts;
        }
        info!("Failed to obtain time from [{}]", site.name);
        // Small delay between retries
        std::thread::sleep(Duration::from_millis(200));
    }

    info!("All server attempts failed, using local time");
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn get_website_timestamp(url: &str) -> Result<i64, ()> {
    // ureq head request
    let resp = ureq::head(url)
        .timeout(Duration::from_secs(6))
        .set(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        )
        .call()
        .map_err(|_| ())?;

    let date_header = resp.header("Date").ok_or(())?;

    // Parse RFC1123 date using chrono
    match chrono::DateTime::parse_from_rfc2822(date_header) {
        Ok(dt) => Ok(dt.timestamp_millis()),
        Err(_) => Err(()),
    }
}
