// Application constants definition
#[allow(dead_code)]
pub const APP_NAME: &str = "WSL Dashboard";
#[allow(dead_code)]
pub const APP_ID: &str = "wsldashboard";

#[allow(dead_code)]
pub const COMPANY_NAME: &str = APP_NAME;
#[allow(dead_code)]
pub const LEGAL_COPYRIGHT: &str = "2026 WSL Dashboard. All rights reserved.";

// FREE
#[allow(dead_code)]
pub const GITHUB_URL: &str = "https://github.com/owu/wsl-dashboard";

// CN
#[allow(dead_code)]
pub const GITEE_URL: &str = "https://gitee.com/bye/wsl-dashboard";

#[allow(dead_code)]
pub const GITHUB_ISSUES: &str = "/issues";

#[allow(dead_code)]
pub const GITHUB_RELEASES: &str = "/releases";

// FREE
#[allow(dead_code)]
pub const STATIC_API_FREE: &str = "https://raw.githubusercontent.com/owu/oss/refs/heads";

// CN
#[allow(dead_code)]
pub const STATIC_API: &str = "https://gitee.com/bye/oss/raw";

#[allow(dead_code)]
pub const BASE_API: &str = "/main/wsldashboard/api/base.json";

#[allow(dead_code)]
pub const INSTANCE_API: &str = "/main/wsldashboard/api/instance.json";

#[allow(dead_code)]
pub const ZH_TIMEZONE: &str = "UTC+08:00";

// Compatibility of Chinese and Japanese character display on Western language operating systems
// Font constants
#[allow(dead_code)]
pub const FONT_ZH: &str = "Microsoft YaHei UI";
#[allow(dead_code)]
pub const FONT_EN_FALLBACK: &str = "Segoe UI, Microsoft YaHei UI";

/// Check if a language code represents Chinese
#[allow(dead_code)]
pub fn is_chinese_lang(lang: &str) -> bool {
    lang.to_lowercase().starts_with("zh")
}

/// Check if a language code represents Simplified Chinese (zh-CN only)
#[allow(dead_code)]
pub fn is_simplified_chinese(lang: &str) -> bool {
    lang.eq_ignore_ascii_case("zh-cn")
}

/// WSL distribution initialization script path
#[allow(dead_code)]
pub const WSL_INIT_SCRIPT: &str = "/etc/init.wsl-dashboard";

/// Maximum length for WSL instance names
#[allow(dead_code)]
pub const MAX_INSTANCE_NAME_LEN: usize = 32;

/// Default mirror source display name
#[allow(dead_code)]
pub const DEFAULT_MIRROR_NAME: &str = "源地址(默认)";

/// Mirror source display names (zh-CN only)
#[allow(dead_code)]
pub const MIRROR_NAMES: &[&str] = &[
    DEFAULT_MIRROR_NAME,
    "gh-proxy.com",
    "gh.ddlc.top",
    "ghproxy.vip",
    "ghfast.top",
    "ghproxy.net",
    "自定义源",
];

/// Mirror source URL prefixes (parallel to MIRROR_NAMES)
#[allow(dead_code)]
pub const MIRROR_PREFIXES: &[&str] = &[
    "",
    "https://gh-proxy.com/",
    "https://gh.ddlc.top/",
    "https://ghproxy.vip/",
    "https://ghfast.top/",
    "https://ghproxy.net/",
    "", // custom - handled separately
];

/// Check if a URL is a GitHub URL that can be accelerated by mirror proxy
#[allow(dead_code)]
pub fn is_github_url(url: &str) -> bool {
    url.contains("github.com")
        || url.contains("raw.githubusercontent.com")
        || url.contains("gist.githubusercontent.com")
        || url.contains("api.github.com")
}

/// Check if a mirror index is a valid accelerator (non-default, non-custom)
#[allow(dead_code)]
pub fn is_accelerator_mirror(idx: usize) -> bool {
    idx > 0 && idx < MIRROR_NAMES.len() - 1
}

/// Apply mirror prefix to URL. Returns wrapped URL if applicable.
#[allow(dead_code)]
pub fn apply_mirror(url: &str, mirror_idx: usize, custom_prefix: &str) -> String {
    if !is_github_url(url) {
        return url.to_string();
    }
    let custom_idx = MIRROR_NAMES.len() - 1;
    match mirror_idx {
        0 => url.to_string(), // source
        _ if mirror_idx == custom_idx => {
            let prefix = custom_prefix.trim();
            if prefix.is_empty() {
                url.to_string()
            } else {
                format!("{}{}", prefix, url)
            }
        }
        _ if is_accelerator_mirror(mirror_idx) => {
            format!("{}{}", MIRROR_PREFIXES[mirror_idx], url)
        }
        _ => url.to_string(),
    }
}

/// Domain mirror display names (for Ubuntu/Fedora/archlinux)
#[allow(dead_code)]
pub const DOMAIN_MIRROR_NAMES: &[&str] = &[DEFAULT_MIRROR_NAME, "清华", "中科大", "阿里云"];

/// Tsinghua mirror domain replacement rules
#[allow(dead_code)]
const TSINGHUA_RULES: &[(&str, &str)] = &[
    (
        "releases.ubuntu.com/",
        "mirrors.tuna.tsinghua.edu.cn/ubuntu-releases/",
    ),
    (
        "cdimages.ubuntu.com/",
        "mirrors.tuna.tsinghua.edu.cn/ubuntu-cdimage/ubuntu/",
    ),
    (
        "cdimage.ubuntu.com/",
        "mirrors.tuna.tsinghua.edu.cn/ubuntu-cdimage/",
    ),
    (
        "download.fedoraproject.org/pub/fedora/linux/",
        "mirrors.tuna.tsinghua.edu.cn/fedora/",
    ),
    (
        "fastly.mirror.pkgbuild.com/",
        "mirrors.tuna.tsinghua.edu.cn/archlinux/",
    ),
];

/// USTC mirror domain replacement rules
#[allow(dead_code)]
const USTC_RULES: &[(&str, &str)] = &[
    (
        "releases.ubuntu.com/",
        "mirrors.ustc.edu.cn/ubuntu-releases/",
    ),
    (
        "cdimages.ubuntu.com/",
        "mirrors.ustc.edu.cn/ubuntu-cdimage/",
    ),
    (
        "cdimage.ubuntu.com/ubuntu/",
        "mirrors.ustc.edu.cn/ubuntu-cdimage/",
    ),
    (
        "download.fedoraproject.org/pub/fedora/linux/",
        "mirrors.ustc.edu.cn/fedora/",
    ),
    (
        "fastly.mirror.pkgbuild.com/",
        "mirrors.ustc.edu.cn/archlinux/",
    ),
];

/// Aliyun mirror domain replacement rules
#[allow(dead_code)]
const ALIYUN_RULES: &[(&str, &str)] = &[
    (
        "releases.ubuntu.com/",
        "mirrors.aliyun.com/ubuntu-releases/",
    ),
    (
        "cdimages.ubuntu.com/",
        "mirrors.aliyun.com/ubuntu-cdimage/ubuntu/",
    ),
    ("cdimage.ubuntu.com/", "mirrors.aliyun.com/ubuntu-cdimage/"),
    (
        "download.fedoraproject.org/pub/fedora/linux/",
        "mirrors.aliyun.com/fedora/",
    ),
    (
        "fastly.mirror.pkgbuild.com/",
        "mirrors.aliyun.com/archlinux/",
    ),
];

/// Check if a distro category supports domain-based mirrors
#[allow(dead_code)]
pub fn supports_domain_mirror(category: &str) -> bool {
    matches!(
        category.to_lowercase().as_str(),
        "ubuntu" | "fedora" | "archlinux"
    )
}

/// Return i18n keys for domain mirrors that have at least one matching rule for the given URL.
/// If no mirror has any matching rule, returns just ["mirror.default"].
#[allow(dead_code)]
pub fn get_domain_mirror_names(url: &str) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = Vec::new();
    let checks: &[&[(&str, &str)]] = &[TSINGHUA_RULES, USTC_RULES, ALIYUN_RULES];
    let labels: &[&str] = &["清华", "中科大", "阿里云"];
    for (rules, label) in checks.iter().zip(labels.iter()) {
        if rules.iter().any(|(from, _)| url.contains(from)) {
            names.push(label);
        }
    }
    if names.is_empty() {
        vec![DEFAULT_MIRROR_NAME]
    } else {
        let mut all = vec![DEFAULT_MIRROR_NAME];
        all.extend(names);
        all
    }
}

/// Apply domain-based mirror replacement to a download URL.
/// mirror_idx: 0 = default, 1 = Tsinghua, 2 = USTC, 3 = Aliyun
/// Returns the mirrored URL, or the original URL if no rule matches.
#[allow(dead_code)]
pub fn apply_domain_mirror(url: &str, mirror_idx: usize) -> String {
    if mirror_idx == 0 {
        return url.to_string();
    }
    let rules: &[(&str, &str)] = match mirror_idx {
        1 => TSINGHUA_RULES,
        2 => USTC_RULES,
        3 => ALIYUN_RULES,
        _ => return url.to_string(),
    };
    for (from, to) in rules {
        if url.contains(from) {
            return url.replacen(from, to, 1);
        }
    }
    url.to_string()
}
