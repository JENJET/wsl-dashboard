use std::process::Stdio;
use tracing::{debug, info, warn};

use crate::wsl::executor::{WslCommandExecutor, new_tokio_wsl_cmd};

#[derive(Debug, Clone, Default)]
pub struct MirrorConfig {
    pub apt_mirror: String,
    pub dnf_mirror: String,
    pub pacman_mirror: String,
}

pub async fn get_distro_category(executor: &WslCommandExecutor, distro_name: &str) -> String {
    let result = executor
        .execute_command(&["-d", distro_name, "-e", "cat", "/etc/os-release"])
        .await;

    if !result.success {
        warn!("Failed to read /etc/os-release for '{}'", distro_name);
        return "unknown".to_string();
    }

    let content = result.output.to_lowercase();

    if content.contains("ubuntu") || content.contains("linuxmint") {
        "ubuntu".to_string()
    } else if content.contains("\nid=debian") || content.starts_with("id=debian") {
        "debian".to_string()
    } else if content.contains("kali") {
        "kali".to_string()
    } else if content.contains("almalinux") {
        "almalinux".to_string()
    } else if content.contains("rocky") {
        "rocky".to_string()
    } else if content.contains("centos") || content.contains("rhel") {
        "centos".to_string()
    } else if content.contains("fedora") {
        "fedora".to_string()
    } else if content.contains("arch") || content.contains("manjaro") {
        "archlinux".to_string()
    } else if content.contains("opensuse") || content.contains("sles") {
        "opensuse".to_string()
    } else {
        "unknown".to_string()
    }
}

fn mirror_url_for(category: &str, mirror: &str) -> Option<&'static str> {
    match (category, mirror) {
        ("ubuntu", "tsinghua") => Some("https://mirrors.tuna.tsinghua.edu.cn/ubuntu/"),
        ("ubuntu", "ustc") => Some("https://mirrors.ustc.edu.cn/ubuntu/"),
        ("ubuntu", "aliyun") => Some("https://mirrors.aliyun.com/ubuntu/"),
        ("ubuntu", "tencent") => Some("https://mirrors.cloud.tencent.com/ubuntu/"),
        ("ubuntu", "huawei") => Some("https://repo.huaweicloud.com/ubuntu/"),
        ("debian", "tsinghua") => Some("https://mirrors.tuna.tsinghua.edu.cn/debian/"),
        ("debian", "ustc") => Some("https://mirrors.ustc.edu.cn/debian/"),
        ("debian", "aliyun") => Some("https://mirrors.aliyun.com/debian/"),
        ("debian", "tencent") => Some("https://mirrors.cloud.tencent.com/debian/"),
        ("debian", "huawei") => Some("https://repo.huaweicloud.com/debian/"),
        ("kali", "tsinghua") => Some("https://mirrors.tuna.tsinghua.edu.cn/kali/"),
        ("kali", "ustc") => Some("https://mirrors.ustc.edu.cn/kali/"),
        ("kali", "aliyun") => Some("https://mirrors.aliyun.com/kali/"),
        ("kali", "tencent") => Some("https://mirrors.cloud.tencent.com/kali/"),
        ("kali", "huawei") => Some("https://repo.huaweicloud.com/kali/"),
        ("opensuse", "tsinghua") => Some("https://mirrors.tuna.tsinghua.edu.cn/opensuse/"),
        ("opensuse", "ustc") => Some("https://mirrors.ustc.edu.cn/opensuse/"),
        ("opensuse", "aliyun") => Some("https://mirrors.aliyun.com/opensuse/"),
        ("opensuse", "tencent") => Some("https://mirrors.cloud.tencent.com/opensuse/"),
        ("opensuse", "huawei") => Some("https://repo.huaweicloud.com/opensuse/"),
        ("centos", "tsinghua") => Some("https://mirrors.tuna.tsinghua.edu.cn/centos/"),
        ("centos", "ustc") => None,
        ("centos", "aliyun") => Some("https://mirrors.aliyun.com/centos/"),
        ("centos", "tencent") => Some("https://mirrors.cloud.tencent.com/centos/"),
        ("centos", "huawei") => Some("https://repo.huaweicloud.com/centos/"),
        ("fedora", "tsinghua") => Some("https://mirrors.tuna.tsinghua.edu.cn/fedora/"),
        ("fedora", "ustc") => Some("https://mirrors.ustc.edu.cn/fedora/"),
        ("fedora", "aliyun") => Some("https://mirrors.aliyun.com/fedora/"),
        ("fedora", "tencent") => Some("https://mirrors.cloud.tencent.com/fedora/"),
        ("fedora", "huawei") => Some("https://repo.huaweicloud.com/fedora/"),
        ("rocky", "tsinghua") => None,
        ("rocky", "ustc") => Some("https://mirrors.ustc.edu.cn/rocky/"),
        ("rocky", "aliyun") => Some("https://mirrors.aliyun.com/rockylinux/"),
        ("rocky", "tencent") => Some("https://mirrors.cloud.tencent.com/rocky/"),
        ("rocky", "huawei") => Some("https://repo.huaweicloud.com/rockylinux/"),
        ("almalinux", "tsinghua") | ("almalinux", "ustc") => None,
        ("almalinux", "aliyun") => Some("https://mirrors.aliyun.com/almalinux/"),
        ("almalinux", "tencent") => Some("https://mirrors.cloud.tencent.com/almalinux/"),
        ("almalinux", "huawei") => Some("https://repo.huaweicloud.com/almalinux/"),
        ("archlinux", "tsinghua") => Some("https://mirrors.tuna.tsinghua.edu.cn/archlinux/"),
        ("archlinux", "ustc") => Some("https://mirrors.ustc.edu.cn/archlinux/"),
        ("archlinux", "aliyun") => Some("https://mirrors.aliyun.com/archlinux/"),
        ("archlinux", "tencent") => Some("https://mirrors.cloud.tencent.com/archlinux/"),
        ("archlinux", "huawei") => Some("https://repo.huaweicloud.com/archlinux/"),
        _ => None,
    }
}

fn mirror_domain_for(mirror: &str) -> Option<&'static str> {
    match mirror {
        "tsinghua" => Some("mirrors.tuna.tsinghua.edu.cn"),
        "ustc" => Some("mirrors.ustc.edu.cn"),
        "aliyun" => Some("mirrors.aliyun.com"),
        "tencent" => Some("mirrors.cloud.tencent.com"),
        "huawei" => Some("repo.huaweicloud.com"),
        _ => None,
    }
}

async fn uses_deb822_format(executor: &WslCommandExecutor, distro_name: &str) -> String {
    // 返回 deb822 文件路径（任意 *.sources 文件），空字符串表示无 deb822 文件
    let result = executor
        .execute_command(&[
            "-d",
            distro_name,
            "-e",
            "sh",
            "-c",
            "ls /etc/apt/sources.list.d/*.sources 2>/dev/null | head -1",
        ])
        .await;
    if result.success {
        result.output.trim().to_string()
    } else {
        String::new()
    }
}

fn gen_deb822_sources(mirror_url: &str, codename: &str, category: &str) -> String {
    let (components, keyring) = if category == "debian" {
        (
            "main contrib non-free non-free-firmware",
            "/usr/share/keyrings/debian-archive-keyring.gpg",
        )
    } else {
        (
            "main restricted universe multiverse",
            "/usr/share/keyrings/ubuntu-archive-keyring.gpg",
        )
    };
    format!(
        "Types: deb\n\
         URIs: {mirror_url}\n\
         Suites: {codename} {codename}-updates {codename}-backports\n\
         Components: {components}\n\
         Signed-By: {keyring}\n\
         \n\
         Types: deb-src\n\
         URIs: {mirror_url}\n\
         Suites: {codename} {codename}-updates {codename}-backports\n\
         Components: {components}\n\
         Signed-By: {keyring}\n"
    )
}

async fn detect_codename(executor: &WslCommandExecutor, distro_name: &str) -> String {
    let result = executor
        .execute_command(&[
            "-d",
            distro_name,
            "-e",
            "sh",
            "-c",
            ". /etc/os-release && echo \"$VERSION_CODENAME\"",
        ])
        .await;
    if result.success {
        result.output.trim().to_string()
    } else {
        "noble".to_string()
    }
}

fn gen_traditional_ubuntu_sources(mirror_url: &str, codename: &str) -> String {
    format!(
        "deb {mirror_url} {codename} main restricted universe multiverse\n\
         deb {mirror_url} {codename}-updates main restricted universe multiverse\n\
         deb {mirror_url} {codename}-backports main restricted universe multiverse\n\
         deb-src {mirror_url} {codename} main restricted universe multiverse\n\
         deb-src {mirror_url} {codename}-updates main restricted universe multiverse\n\
         deb-src {mirror_url} {codename}-backports main restricted universe multiverse\n"
    )
}

fn gen_debian_traditional_sources(mirror_url: &str, codename: &str) -> String {
    format!(
        "deb {mirror_url} {codename} main contrib non-free non-free-firmware\n\
         deb {mirror_url} {codename}-updates main contrib non-free non-free-firmware\n\
         deb {mirror_url} {codename}-backports main contrib non-free non-free-firmware\n\
         deb-src {mirror_url} {codename} main contrib non-free non-free-firmware\n\
         deb-src {mirror_url} {codename}-updates main contrib non-free non-free-firmware\n\
         deb-src {mirror_url} {codename}-backports main contrib non-free non-free-firmware\n"
    )
}

fn replace_traditional_mirror(content: &str, mirror_url: &str) -> String {
    let known_domains = [
        "archive.ubuntu.com",
        "security.ubuntu.com",
        "ports.ubuntu.com",
        "deb.debian.org",
        "httpredir.debian.org",
        "ftp.debian.org",
        "security.debian.org",
        "cdn.debian.net",
        "deb.uh-oh.com",
    ];
    let mut out = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("deb ") || t.starts_with("deb-src ") {
            let parts: Vec<&str> = t.splitn(3, ' ').collect();
            if parts.len() >= 3 && known_domains.iter().any(|d| parts[1].contains(d)) {
                out.push(format!("{} {} {}", parts[0], mirror_url, parts[2]));
            } else {
                out.push(line.to_string());
            }
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n")
}

pub async fn apply_apt_mirror(
    executor: &WslCommandExecutor,
    distro_name: &str,
    category: &str,
    mirror: &str,
) -> Result<(), String> {
    if mirror.is_empty() {
        // 还原备份
        for f in &[
            "/etc/apt/sources.list",
            "/etc/apt/sources.list.d/ubuntu.sources",
            "/etc/apt/sources.list.d/debian.sources",
        ] {
            let cmd = format!("[ -f {f}.bak ] && cp {f}.bak {f} || true", f = f);
            let _ = executor
                .execute_command(&["-d", distro_name, "-u", "root", "-e", "sh", "-c", &cmd])
                .await;
        }
        return Ok(());
    }

    let mirror_url = mirror_url_for(category, mirror)
        .ok_or_else(|| format!("不支持的镜像源: {} / {}", category, mirror))?;

    let deb822_file = uses_deb822_format(executor, distro_name).await;
    let codename = detect_codename(executor, distro_name).await;

    if !deb822_file.is_empty() {
        let target = deb822_file.as_str();
        // 仅首次备份
        let _ = executor
            .execute_command(&[
                "-d",
                distro_name,
                "-u",
                "root",
                "-e",
                "sh",
                "-c",
                &format!(
                    "[ -f {t}.bak ] || cp {t} {t}.bak 2>/dev/null || true",
                    t = target
                ),
            ])
            .await;

        let content = gen_deb822_sources(&mirror_url, &codename, category);
        let write_cmd = format!(
            "cat << 'MIRROREOF' > {}\n{}\nMIRROREOF",
            target,
            content.trim_end()
        );
        let r = executor
            .execute_command(&[
                "-d",
                distro_name,
                "-u",
                "root",
                "-e",
                "sh",
                "-c",
                &write_cmd,
            ])
            .await;
        if !r.success {
            return Err(format!("写入 {} 失败: {}", target, r.output));
        }
    } else {
        let _ = executor
            .execute_command(&[
                "-d",
                distro_name,
                "-u",
                "root",
                "-e",
                "sh",
                "-c",
                "[ -f /etc/apt/sources.list.bak ] || cp /etc/apt/sources.list /etc/apt/sources.list.bak 2>/dev/null || true",
            ])
            .await;

        let current = executor
            .execute_command(&[
                "-d",
                distro_name,
                "-u",
                "root",
                "-e",
                "cat",
                "/etc/apt/sources.list",
            ])
            .await;

        let new_content = if current.success && !current.output.trim().is_empty() {
            replace_traditional_mirror(&current.output, &mirror_url)
        } else if category == "debian" {
            gen_debian_traditional_sources(&mirror_url, &codename)
        } else {
            gen_traditional_ubuntu_sources(&mirror_url, &codename)
        };

        let write_cmd = format!(
            "cat << 'MIRROREOF' > /etc/apt/sources.list\n{}\nMIRROREOF",
            new_content.trim_end()
        );
        let r = executor
            .execute_command(&[
                "-d",
                distro_name,
                "-u",
                "root",
                "-e",
                "sh",
                "-c",
                &write_cmd,
            ])
            .await;
        if !r.success {
            return Err(format!("写入 sources.list 失败: {}", r.output));
        }
    }

    Ok(())
}

async fn verify_apt(distro_name: &str, mirror: &str) -> Result<(), String> {
    let domain = mirror_domain_for(mirror).unwrap_or("");
    if domain.is_empty() {
        return Ok(());
    }
    verify_url_reachable(distro_name, &format!("https://{}/", domain), domain).await
}

async fn verify_url_reachable(distro_name: &str, url: &str, _domain: &str) -> Result<(), String> {
    // 优先 curl，回退 wget
    let r = verify_http_cmd(
        distro_name,
        "curl",
        &[
            "-sI",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            "--connect-timeout",
            "10",
            url,
        ],
    )
    .await;
    if r.is_ok() {
        return r;
    }

    let r = verify_http_cmd(
        distro_name,
        "wget",
        &["--spider", "-q", "-O", "/dev/null", "--timeout=10", url],
    )
    .await;
    if r.is_ok() {
        return r;
    }

    warn!("{}: curl 和 wget 均不可用，跳过验证", distro_name);
    Ok(())
}

async fn verify_http_cmd(distro_name: &str, cmd: &str, args: &[&str]) -> Result<(), String> {
    let mut full = new_tokio_wsl_cmd();
    let mut all = vec!["-d", distro_name, "-e", cmd];
    all.extend_from_slice(args);
    full.args(&all)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = full.output().await.map_err(|_| format!("{} 不可用", cmd))?;
    if !output.status.success() {
        return Err(format!("{} 退出码: {}", cmd, output.status));
    }
    let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if out == "200" || out == "301" || out == "302" {
        info!("Mirror verified via {} (HTTP {})", cmd, out);
        Ok(())
    } else {
        Err(format!("{} 返回 HTTP {}", cmd, out))
    }
}

async fn verify_dnf(distro_name: &str, mirror: &str) -> Result<(), String> {
    let domain = mirror_domain_for(mirror).unwrap_or("");
    if domain.is_empty() {
        return Ok(());
    }
    // dnf makecache / yum makecache 受第三方仓库影响；统一用 curl
    verify_url_reachable(distro_name, &format!("https://{}/", domain), domain).await
}

async fn verify_pacman(distro_name: &str, mirror: &str) -> Result<(), String> {
    let domain = mirror_domain_for(mirror).unwrap_or("");
    if domain.is_empty() {
        return Ok(());
    }
    verify_url_reachable(distro_name, &format!("https://{}/", domain), domain).await
}

pub async fn apply_dnf_mirror(
    executor: &WslCommandExecutor,
    distro_name: &str,
    category: &str,
    mirror: &str,
) -> Result<(), String> {
    if mirror.is_empty() {
        return Ok(());
    }

    match category {
        "centos" | "almalinux" | "rocky" => {
            apply_yum_mirror(executor, distro_name, category, mirror).await
        }
        "fedora" => apply_fedora_mirror(executor, distro_name, mirror).await,
        _ => Err(format!("不支持的发行版类型: {}", category)),
    }
}

async fn apply_yum_mirror(
    executor: &WslCommandExecutor,
    distro_name: &str,
    category: &str,
    mirror: &str,
) -> Result<(), String> {
    if mirror.is_empty() {
        restore_yum_backup(executor, distro_name).await;
        return Ok(());
    }

    let mirror_url = mirror_url_for(category, mirror)
        .ok_or_else(|| format!("{category} 不支持 {mirror} 镜像源"))?;

    let cmd = format!(
        r#"shopt -s nocasematch 2>/dev/null; for f in /etc/yum.repos.d/*.repo; do
            b="$(basename "$f")"
            case "$b" in
                centos*.repo|almalinux*.repo|rocky*.repo)
                    [ -f "$f.bak" ] || cp "$f" "$f.bak" 2>/dev/null || true
                    sed -i 's/^[#[:space:]]*baseurl=/baseurl=/' "$f" 2>/dev/null || true
                    sed -i 's|^baseurl=.*|baseurl={mirror}$releasever/os/$basearch/|' "$f" 2>/dev/null || true
                    sed -i '/^mirrorlist=/ s/^/#/' "$f" 2>/dev/null || true
                    ;;
            esac
        done"#,
        mirror = mirror_url
    );

    let r = executor
        .execute_command(&["-d", distro_name, "-u", "root", "-e", "sh", "-c", &cmd])
        .await;

    if r.success {
        info!(
            "Applied YUM mirror '{}' for '{}' ({})",
            mirror, distro_name, category
        );
        Ok(())
    } else {
        Err(format!("应用 YUM 镜像源失败: {}", r.output))
    }
}

/// 还原 YUM repos 备份
async fn restore_yum_backup(executor: &WslCommandExecutor, distro_name: &str) {
    let cmd = "for f in /etc/yum.repos.d/*.bak; do [ -f \"$f\" ] || continue; cp \"$f\" \"${f%.bak}\" 2>/dev/null || true; done";
    let _ = executor
        .execute_command(&["-d", distro_name, "-u", "root", "-e", "sh", "-c", cmd])
        .await;
}

async fn apply_fedora_mirror(
    executor: &WslCommandExecutor,
    distro_name: &str,
    mirror: &str,
) -> Result<(), String> {
    if mirror.is_empty() {
        restore_yum_backup(executor, distro_name).await;
        return Ok(());
    }

    let mirror_url = match mirror {
        "tsinghua" => "https://mirrors.tuna.tsinghua.edu.cn/fedora/",
        "ustc" => "https://mirrors.ustc.edu.cn/fedora/",
        "aliyun" => "https://mirrors.aliyun.com/fedora/",
        "tencent" => "https://mirrors.cloud.tencent.com/fedora/",
        "huawei" => "https://repo.huaweicloud.com/fedora/",
        _ => return Ok(()),
    };

    let cmd = format!(
        r#"
        for f in fedora.repo fedora-updates.repo fedora-updates-testing.repo; do
            f="/etc/yum.repos.d/$f"
            if [ -f "$f" ]; then
                [ -f "$f.bak" ] || cp "$f" "$f.bak" 2>/dev/null || true
                sed -i '/^metalink=/ s/^/#/' "$f" 2>/dev/null || true
                sed -i '/^mirrorlist=/ s/^/#/' "$f" 2>/dev/null || true
                sed -i 's/^[#[:space:]]*baseurl=/baseurl=/' "$f" 2>/dev/null || true
                sed -i 's|^baseurl=.*|baseurl={mirror}$releasever/Everything/$basearch/os/|' "$f" 2>/dev/null || true
            fi
        done"#,
        mirror = mirror_url
    );

    let r = executor
        .execute_command(&["-d", distro_name, "-u", "root", "-e", "sh", "-c", &cmd])
        .await;

    if r.success {
        info!("Applied Fedora mirror '{}' for '{}'", mirror, distro_name);
        Ok(())
    } else {
        Err(format!("应用 Fedora 镜像源失败: {}", r.output))
    }
}

pub async fn apply_opensuse_mirror(
    executor: &WslCommandExecutor,
    distro_name: &str,
    mirror: &str,
) -> Result<(), String> {
    if mirror.is_empty() {
        let _ = executor
            .execute_command(&["-d", distro_name, "-u", "root", "-e", "sh", "-c",
                "for f in /etc/zypp/repos.d/*.bak; do [ -f \"$f\" ] && cp \"$f\" \"${f%.bak}\" 2>/dev/null || true; done"]).await;
        return Ok(());
    }

    let mirror_url = match mirror {
        "tsinghua" => "https://mirrors.tuna.tsinghua.edu.cn/opensuse",
        "ustc" => "https://mirrors.ustc.edu.cn/opensuse",
        "aliyun" => "https://mirrors.aliyun.com/opensuse",
        "tencent" => "https://mirrors.cloud.tencent.com/opensuse",
        "huawei" => "https://repo.huaweicloud.com/opensuse",
        _ => return Ok(()),
    };

    let cmd = format!(
        r#"
        for f in /etc/zypp/repos.d/*.repo; do
            [ -f "$f" ] || continue
            [ -f "$f.bak" ] || cp "$f" "$f.bak" 2>/dev/null || true
            sed -i "s|https\?://cdn\.opensuse\.org|{mirror}|g" "$f" 2>/dev/null || true
            sed -i "s|https\?://download\.opensuse\.org|{mirror}|g" "$f" 2>/dev/null || true
            echo "OK: $(basename $f)"
        done"#,
        mirror = mirror_url
    );

    let r = executor
        .execute_command(&["-d", distro_name, "-u", "root", "-e", "sh", "-c", &cmd])
        .await;

    if r.success {
        info!("Applied openSUSE mirror '{}' for '{}'", mirror, distro_name);
        Ok(())
    } else {
        Err(format!("应用 openSUSE 镜像源失败: {}", r.output))
    }
}

pub async fn apply_pacman_mirror(
    executor: &WslCommandExecutor,
    distro_name: &str,
    mirror: &str,
) -> Result<(), String> {
    if mirror.is_empty() {
        let _ = executor
            .execute_command(&["-d", distro_name, "-u", "root", "-e", "sh", "-c",
                "[ -f /etc/pacman.d/mirrorlist.bak ] && cp /etc/pacman.d/mirrorlist.bak /etc/pacman.d/mirrorlist || true"]).await;
        return Ok(());
    }

    let mirror_url = match mirror {
        "tsinghua" => "https://mirrors.tuna.tsinghua.edu.cn/archlinux/",
        "ustc" => "https://mirrors.ustc.edu.cn/archlinux/",
        "aliyun" => "https://mirrors.aliyun.com/archlinux/",
        "tencent" => "https://mirrors.cloud.tencent.com/archlinux/",
        "huawei" => "https://repo.huaweicloud.com/archlinux/",
        _ => return Ok(()),
    };

    let _ = executor
        .execute_command(&["-d", distro_name, "-u", "root", "-e", "sh", "-c",
            "[ -f /etc/pacman.d/mirrorlist.bak ] || cp /etc/pacman.d/mirrorlist /etc/pacman.d/mirrorlist.bak 2>/dev/null || true"]).await;

    let write_cmd = format!(
        "echo '## Generated by WSL Dashboard\nServer = {mirror}$repo/os/$arch' > /etc/pacman.d/mirrorlist",
        mirror = mirror_url
    );

    let r = executor
        .execute_command(&[
            "-d",
            distro_name,
            "-u",
            "root",
            "-e",
            "sh",
            "-c",
            &write_cmd,
        ])
        .await;

    if r.success {
        info!("Applied Pacman mirror '{}' for '{}'", mirror, distro_name);
        Ok(())
    } else {
        Err(format!("应用 Pacman 镜像源失败: {}", r.output))
    }
}

pub async fn apply_all_mirrors(
    executor: &WslCommandExecutor,
    distro_name: &str,
    config: &MirrorConfig,
) -> Result<(), String> {
    let category = get_distro_category(executor, distro_name).await;

    match category.as_str() {
        "ubuntu" | "debian" | "kali" => {
            apply_apt_mirror(executor, distro_name, &category, &config.apt_mirror).await?;
            if !config.apt_mirror.is_empty() {
                verify_apt(distro_name, &config.apt_mirror).await?;
            }
        }
        "centos" | "almalinux" | "rocky" | "fedora" => {
            apply_dnf_mirror(executor, distro_name, &category, &config.dnf_mirror).await?;
            if !config.dnf_mirror.is_empty() {
                verify_dnf(distro_name, &config.dnf_mirror).await?;
            }
        }
        "archlinux" => {
            apply_pacman_mirror(executor, distro_name, &config.pacman_mirror).await?;
            if !config.pacman_mirror.is_empty() {
                verify_pacman(distro_name, &config.pacman_mirror).await?;
            }
        }
        "opensuse" => {
            apply_opensuse_mirror(executor, distro_name, &config.dnf_mirror).await?;
            if !config.dnf_mirror.is_empty() {
                verify_dnf(distro_name, &config.dnf_mirror).await?;
            }
        }
        _ => {
            debug!(
                "Unknown distribution type '{}', skipping mirror configuration",
                category
            );
            return Err(format!("无法识别的发行版类型: {}", category));
        }
    }

    Ok(())
}
