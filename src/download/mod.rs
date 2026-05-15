use crate::i18n;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub struct FlatDistroEntry {
    pub name: String,
    pub friendly_name: String,
    pub amd64_url: String,
    pub amd64_sha256: String,
    pub arm64_url: String,
    pub arm64_sha256: String,
}

/// Parse DistributionInfo JSON string into a flat list of distros with their URLs.
/// Preserves the original JSON order of categories and distributions.
/// Each entry contains both Amd64Url and Arm64Url if available.
/// Returns (entries, default_index) where default_index is the index of the default distro.
pub fn parse_distribution_info(
    json_str: &str,
) -> Result<(Vec<FlatDistroEntry>, Option<usize>), String> {
    use serde::Deserialize;

    /// Root structure of DistributionInfo.json, with ordered ModernDistributions
    #[derive(Deserialize)]
    struct DistroInfoRoot {
        #[serde(rename = "Default")]
        default: Option<String>,
        #[serde(
            rename = "ModernDistributions",
            deserialize_with = "deserialize_ordered_object"
        )]
        modern_distributions: Vec<(String, Vec<serde_json::Value>)>,
    }

    let root: DistroInfoRoot =
        serde_json::from_str(json_str).map_err(|e| format!("Failed to parse JSON: {}", e))?;

    let default_category = root.default.as_deref();
    let categories = root.modern_distributions;

    let mut entries = Vec::new();
    let mut default_index: Option<usize> = None;

    for (category_name, distros) in categories {
        let is_default_category = default_category.map_or(false, |d| d == category_name);

        for d in &distros {
            let name = d.get("Name").and_then(|v| v.as_str()).unwrap_or("");
            let friendly_name = d.get("FriendlyName").and_then(|v| v.as_str()).unwrap_or("");
            let is_default_entry = d.get("Default").and_then(|v| v.as_bool()).unwrap_or(false);

            let entry_idx = entries.len();

            let amd64 = parse_url_entry(d, "Amd64Url");
            let arm64 = parse_url_entry(d, "Arm64Url");

            // Only include entries that have at least one architecture URL
            if amd64.is_some() || arm64.is_some() {
                entries.push(FlatDistroEntry {
                    name: name.to_string(),
                    friendly_name: friendly_name.to_string(),
                    amd64_url: amd64.as_ref().map(|u| u.0.clone()).unwrap_or_default(),
                    amd64_sha256: amd64.as_ref().map(|u| u.1.clone()).unwrap_or_default(),
                    arm64_url: arm64.as_ref().map(|u| u.0.clone()).unwrap_or_default(),
                    arm64_sha256: arm64.as_ref().map(|u| u.1.clone()).unwrap_or_default(),
                });
                if is_default_category && default_index.is_none() {
                    if is_default_entry || default_index.is_none() {
                        default_index = Some(entry_idx);
                    }
                }
            }
        }
    }

    if entries.is_empty() {
        return Err("No distributions found in JSON".to_string());
    }

    Ok((entries, default_index.or(Some(0))))
}

/// Custom deserialization: parse a JSON object into an ordered Vec<(K, V)>,
/// preserving the original key order from the document instead of using BTreeMap.
fn deserialize_ordered_object<'de, D>(
    deserializer: D,
) -> Result<Vec<(String, Vec<serde_json::Value>)>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, MapAccess, Visitor};
    use std::fmt;

    struct OrderedObjectVisitor;

    impl<'de> Visitor<'de> for OrderedObjectVisitor {
        type Value = Vec<(String, Vec<serde_json::Value>)>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a JSON object with array values")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut result = Vec::new();
            while let Some(key) = map.next_key::<String>()? {
                let value: serde_json::Value = map.next_value()?;
                let arr = value.as_array().cloned().ok_or_else(|| {
                    de::Error::custom(format!("Expected array for category '{}'", key))
                })?;
                result.push((key, arr));
            }
            Ok(result)
        }
    }

    deserializer.deserialize_map(OrderedObjectVisitor)
}

fn parse_url_entry(distro: &serde_json::Value, key: &str) -> Option<(String, String)> {
    let url_entry = distro.get(key)?;
    let url = url_entry.get("Url")?.as_str()?.to_string();
    let sha256 = url_entry.get("Sha256")?.as_str()?.to_string();
    Some((url, sha256))
}

/// Parse SHA256 string (may have 0x prefix)
fn parse_sha256(hex_str: &str) -> Result<Vec<u8>, String> {
    let hex_str = hex_str.trim();
    let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    hex::decode(hex_str).map_err(|e| format!("Failed to decode SHA256: {}", e))
}

/// Verify a file's SHA256 hash
pub fn verify_sha256(path: &Path, expected_hex: &str) -> bool {
    let expected_bytes = match parse_sha256(expected_hex) {
        Ok(b) => b,
        Err(e) => {
            error!("Failed to parse expected SHA256: {}", e);
            return false;
        }
    };

    if expected_bytes.len() != 32 {
        error!(
            "Expected SHA256 hash is {} bytes, expected 32",
            expected_bytes.len()
        );
        return false;
    }

    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            error!("Failed to open file for SHA256 verification: {}", e);
            return false;
        }
    };

    let mut hasher = Sha256::new();
    let mut reader = std::io::BufReader::new(file);
    let mut buffer = [0u8; 65536];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buffer[..n]),
            Err(e) => {
                error!("Failed to read file for SHA256: {}", e);
                return false;
            }
        }
    }
    let hash = hasher.finalize();
    hash[..] == expected_bytes[..]
}

/// Async version: runs verify_sha256 on a blocking thread
pub async fn verify_sha256_async(path: PathBuf, expected_hex: String) -> bool {
    tokio::task::spawn_blocking(move || verify_sha256(&path, &expected_hex))
        .await
        .unwrap_or(false)
}

// ─── Download Manager ───

pub struct DownloadManager {
    cache_dir: PathBuf,
}

impl DownloadManager {
    pub fn new(cache_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&cache_dir).ok();
        Self { cache_dir }
    }

    fn filename_for_label(&self, _label: &str, sha256: &str, url: &str) -> String {
        let clean = sha256.trim_start_matches("0x");
        let prefix = &clean[..8.min(clean.len())];
        let suffix = &clean[clean.len().saturating_sub(8)..];
        // Extract filename from URL (last path segment, strip query params)
        let url_name = url.split('/').last().unwrap_or("");
        let url_name = url_name.split('?').next().unwrap_or(url_name);
        let url_name = url_name.split('#').next().unwrap_or(url_name);
        let safe_name: String = url_name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let basename = if safe_name.is_empty() {
            "distro"
        } else {
            &safe_name
        };
        // Insert hash between stem and extension:  name.prefix.suffix.ext
        if let Some(dot) = basename.rfind('.') {
            let stem = &basename[..dot];
            let ext = &basename[dot..];
            format!("{}.{}.{}{}", stem, prefix, suffix, ext)
        } else {
            format!("{}.{}.{}", basename, prefix, suffix)
        }
    }

    fn get_cache_path(&self, label: &str, sha256: &str, url: &str) -> PathBuf {
        self.cache_dir
            .join(self.filename_for_label(label, sha256, url))
    }

    fn get_partial_path(&self, label: &str, sha256: &str, url: &str) -> PathBuf {
        let name = self.filename_for_label(label, sha256, url);
        self.cache_dir.join(format!("{}.part", name))
    }

    fn get_chunks_dir(&self, label: &str, sha256: &str, url: &str) -> PathBuf {
        let name = self.filename_for_label(label, sha256, url);
        self.cache_dir.join(format!("{}.part.chunks", name))
    }

    fn count_chunks(dir: &Path) -> Option<usize> {
        let entries: Vec<_> = std::fs::read_dir(dir)
            .ok()?
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        let count = entries
            .iter()
            .filter(|e| e.file_name().to_string_lossy().starts_with("chunk_"))
            .count();
        if count > 0 { Some(count) } else { None }
    }

    fn sum_chunks(dir: &Path) -> u64 {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return 0,
        };
        entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with("chunk_") {
                    e.metadata().ok().map(|m| m.len())
                } else {
                    None
                }
            })
            .sum()
    }

    /// Download a file with multi-threaded support, resume, and SHA256 verification.
    /// `label` is used for cache filename (e.g. distro friendly name).
    /// `progress_callback(current_bytes, total_bytes)`: called with download progress.
    /// `status_callback(msg)`: called with text status updates (e.g. retry info).
    /// Returns the path to the cached file on success.
    pub async fn download(
        &self,
        label: &str,
        url: &str,
        expected_sha256: &str,
        threads: usize,
        max_retries: usize,
        progress_callback: Option<impl Fn(u64, u64) + Send + 'static>,
        status_callback: Option<impl Fn(String) + Send + 'static>,
    ) -> Result<PathBuf, String> {
        let final_path = self.get_cache_path(label, expected_sha256, url);
        let partial_path = self.get_partial_path(label, expected_sha256, url);

        let threads = threads.clamp(1, 8);

        info!(
            "download: final={:?}, partial={:?}, sha256={}, threads={}",
            final_path, partial_path, expected_sha256, threads
        );

        // 1. Check if cached file exists and is valid
        if final_path.exists() {
            info!("[CACHE CHECK] final_path exists, verifying SHA256...");
            if let Some(ref cb) = status_callback {
                cb(format!("verify_start"));
            }
            if verify_sha256_async(final_path.clone(), expected_sha256.to_owned()).await {
                info!("[CACHE HIT] using cached file: {:?}", final_path);
                return Ok(final_path);
            } else {
                warn!("[CACHE MISS] cache file SHA256 mismatch, removing and re-downloading");
                std::fs::remove_file(&final_path).ok();
            }
        } else {
            info!("[CACHE CHECK] final_path does NOT exist");
        }

        // 2. Check .part exists (completed merge but not yet renamed)
        if partial_path.exists() {
            info!("[PARTIAL CHECK] .part exists, verifying SHA256...");
            if let Some(ref cb) = status_callback {
                cb(format!("verify_start"));
            }
            if verify_sha256_async(partial_path.clone(), expected_sha256.to_owned()).await {
                info!("[PARTIAL HIT] .part verified OK, renaming to .wsl");
                std::fs::rename(&partial_path, &final_path)
                    .map_err(|e| format!("Failed to rename .part to .wsl: {}", e))?;
                return Ok(final_path);
            } else {
                warn!("[PARTIAL MISS] .part SHA256 failed, deleting");
                std::fs::remove_file(&partial_path).ok();
            }
        }

        // 3. Check chunks dir thread count
        let chunk_dir = self.get_chunks_dir(label, expected_sha256, url);
        let prev_threads = Self::count_chunks(&chunk_dir);
        if prev_threads.map_or(false, |p| p != threads) {
            info!(
                "[RESUME] thread count changed (prev={:?}, cur={}), clearing chunks",
                prev_threads, threads
            );
            std::fs::remove_dir_all(&chunk_dir).ok();
        }
        // initial_size = sum of existing chunk file sizes (progress display)
        let initial_size = Self::sum_chunks(&chunk_dir);
        if initial_size > 0 {
            info!(
                "[RESUME] {} bytes from {} chunk files",
                initial_size,
                prev_threads.unwrap_or(0)
            );
        } else {
            info!("[RESUME] starting fresh");
        }

        // 4. Get file size from server
        let file_size = get_content_length(url).await?;
        info!("Remote file size: {} bytes", file_size);

        // 4. Retry loop
        let mut last_error = String::new();

        for attempt in 1..=max_retries {
            info!("Download attempt {}/{} for {}", attempt, max_retries, url);

            // Notify UI about retry status
            if attempt > 1 {
                if let Some(ref cb) = status_callback {
                    cb(format!("verify_failed/{}", attempt - 1));
                }
            }

            // Calculate initial bytes from chunk files (not .part)
            let chunk_dir = self.get_chunks_dir(label, expected_sha256, url);
            let current_initial = Self::sum_chunks(&chunk_dir);

            // Report initial progress immediately (important for resume UX)
            if let Some(ref cb) = progress_callback {
                cb(current_initial, file_size);
            }

            let result = self
                .download_chunks(
                    url,
                    file_size,
                    &partial_path,
                    &chunk_dir,
                    threads,
                    current_initial,
                    &progress_callback,
                )
                .await;

            match result {
                Ok(()) => {
                    // Verify SHA256
                    info!("Download complete, verifying SHA256...");
                    if let Some(ref cb) = status_callback {
                        cb(format!("verify_start"));
                    }
                    if verify_sha256_async(partial_path.clone(), expected_sha256.to_owned()).await {
                        info!("SHA256 verification passed");
                        std::fs::remove_dir_all(&chunk_dir).ok();
                        std::fs::rename(&partial_path, &final_path)
                            .map_err(|e| format!("Failed to rename completed file: {}", e))?;
                        return Ok(final_path);
                    } else {
                        warn!(
                            "SHA256 verification failed on attempt {}/{}",
                            attempt, max_retries
                        );
                        last_error = i18n::tr(
                            "install.url.step_1_4_verify_failed",
                            &[attempt.to_string(), max_retries.to_string()],
                        )
                        .to_string();
                        // Delete .part and chunks before retry
                        std::fs::remove_file(&partial_path).ok();
                        std::fs::remove_dir_all(&chunk_dir).ok();
                        if attempt < max_retries {
                            if let Some(cb) = progress_callback.as_ref() {
                                cb(0, file_size);
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Download attempt {}/{} failed: {}", attempt, max_retries, e);
                    // Clean up .part and chunks in case merge failed midway
                    std::fs::remove_file(&partial_path).ok();
                    std::fs::remove_dir_all(&chunk_dir).ok();
                    last_error = e;
                    if attempt >= max_retries {
                        break;
                    }
                }
            }
        }

        Err(format!(
            "Download failed after {} attempts: {}",
            max_retries, last_error
        ))
    }

    async fn download_chunks(
        &self,
        url: &str,
        file_size: u64,
        partial_path: &Path,
        chunk_dir: &Path,
        threads: usize,
        initial_size: u64,
        progress_callback: &Option<impl Fn(u64, u64) + Send + 'static>,
    ) -> Result<(), String> {
        std::fs::create_dir_all(chunk_dir).ok();

        let full_chunk = (file_size + threads as u64 - 1) / threads as u64;
        let total_init = Arc::new(AtomicU64::new(initial_size));
        let url_arc = Arc::new(url.to_string());
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let done_flag = Arc::new(AtomicBool::new(false));
        let mut handles = Vec::new();
        for i in 0..threads {
            let chunk_path = chunk_dir.join(format!("chunk_{}", i));
            let rstart = i as u64 * full_chunk;
            let rend = ((i as u64 + 1) * full_chunk).min(file_size);

            let old_bytes = if chunk_path.exists() {
                std::fs::metadata(&chunk_path).map(|m| m.len()).unwrap_or(0)
            } else {
                0
            };
            let expected = rend - rstart;
            if old_bytes >= expected {
                continue;
            }

            let wstart = rstart + old_bytes;
            if wstart >= rend {
                continue;
            }

            let url = url_arc.clone();
            let dl = total_init.clone();
            let cancel = cancel_flag.clone();
            // Clone values for the closure
            let cp = chunk_path.clone();
            let ws = wstart;
            let re = rend;

            let handle = tokio::task::spawn_blocking(move || {
                let max_retries = 3;
                let mut last_error = String::new();
                for attempt in 1..=max_retries {
                    let current_size = std::fs::metadata(&cp).map(|m| m.len()).unwrap_or(0);
                    let retry_ws = ws + current_size - old_bytes;
                    if retry_ws >= re {
                        return Ok(());
                    }
                    let range = format!("bytes={}-{}", retry_ws, re - 1);
                    let agent = ureq::AgentBuilder::new()
                        .timeout_connect(std::time::Duration::from_secs(30))
                        .timeout_read(std::time::Duration::from_secs(120))
                        .build();
                    let result = (|| {
                        let response =
                            agent.get(&url).set("Range", &range).call().map_err(|e| {
                                format!("Chunk {}-{} failed: {}", retry_ws, re - 1, e)
                            })?;
                        if retry_ws > 0 && response.status() != 206 {
                            return Err(format!(
                                "Server HTTP {} for Range request",
                                response.status()
                            ));
                        }
                        let mut body = response.into_reader().take(re - retry_ws);
                        let mut file = std::fs::OpenOptions::new()
                            .create(true)
                            .write(true)
                            .append(true)
                            .open(&cp)
                            .map_err(|e| format!("Open chunk: {}", e))?;
                        let mut buf = [0u8; 65536];
                        loop {
                            match body.read(&mut buf) {
                                Ok(0) => break,
                                Ok(n) => {
                                    file.write_all(&buf[..n])
                                        .map_err(|e| format!("Write chunk: {}", e))?;
                                    file.flush().ok();
                                    dl.fetch_add(n as u64, Ordering::Relaxed);
                                }
                                Err(e) => {
                                    return Err(format!("Read chunk: {}", e));
                                }
                            }
                        }
                        file.flush().ok();
                        Ok(())
                    })();
                    match result {
                        Ok(()) => return Ok(()),
                        Err(e) => {
                            last_error = format!("{} (attempt {}/{})", e, attempt, max_retries);
                            if attempt < max_retries {
                                continue;
                            }
                            cancel.store(true, Ordering::Relaxed);
                            return Err(last_error);
                        }
                    }
                }
                cancel.store(true, Ordering::Relaxed);
                Err(last_error)
            });
            handles.push(handle);
        }

        total_init.store(initial_size, Ordering::Relaxed);

        let dl_progress = total_init.clone();
        let dl_done = done_flag.clone();

        let (chunks_result, _) = tokio::join!(
            async {
                let mut results = Vec::new();
                for handle in handles {
                    match handle.await {
                        Ok(Ok(())) => results.push(Ok(())),
                        Ok(Err(e)) => {
                            cancel_flag.store(true, Ordering::Relaxed);
                            results.push(Err(e));
                        }
                        Err(e) => {
                            cancel_flag.store(true, Ordering::Relaxed);
                            results.push(Err(format!("Join error: {}", e)));
                        }
                    }
                }
                done_flag.store(true, Ordering::Relaxed);
                results
            },
            async {
                if progress_callback.is_none() {
                    return;
                }
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let cur = dl_progress.load(Ordering::Relaxed);
                    if let Some(cb) = progress_callback.as_ref() {
                        cb(cur, file_size);
                    }
                    if dl_done.load(Ordering::Relaxed) {
                        break;
                    }
                }
            },
        );

        if let Some(cb) = progress_callback.as_ref() {
            cb(total_init.load(Ordering::Relaxed), file_size);
        }

        for result in &chunks_result {
            if let Err(e) = result {
                return Err(e.clone());
            }
        }

        // Merge: rebuild .part from ALL chunk files at their correct offsets
        {
            let mut out = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(partial_path)
                .map_err(|e| format!("Create .part: {}", e))?;
            for i in 0..threads {
                let chunk_path = chunk_dir.join(format!("chunk_{}", i));
                if !chunk_path.exists() {
                    return Err(format!("Missing chunk {} for merge, retrying", i));
                }
                let range_end = ((i as u64 + 1) * full_chunk).min(file_size);
                let expected_len = range_end - (i as u64 * full_chunk);
                let file_len = std::fs::metadata(&chunk_path).map(|m| m.len()).unwrap_or(0);
                if file_len < expected_len {
                    return Err(format!(
                        "Chunk {} too short: {} < {}",
                        i, file_len, expected_len
                    ));
                }
                let pos = i as u64 * full_chunk;
                out.seek(SeekFrom::Start(pos))
                    .map_err(|e| format!("Seek .part: {}", e))?;
                let mut chunk =
                    std::fs::File::open(&chunk_path).map_err(|e| format!("Open chunk: {}", e))?;
                let mut remain = expected_len;
                let mut buf = [0u8; 65536];
                while remain > 0 {
                    let to_read = buf.len().min(remain as usize);
                    match chunk.read(&mut buf[..to_read]) {
                        Ok(0) => break,
                        Ok(n) => {
                            out.write_all(&buf[..n])
                                .map_err(|e| format!("Write .part: {}", e))?;
                            remain -= n as u64;
                        }
                        Err(e) => return Err(format!("Read chunk: {}", e)),
                    }
                }
            }
            out.flush().ok();
        }

        if let Ok(meta) = std::fs::metadata(partial_path) {
            if meta.len() != file_size {
                return Err(format!(
                    "File size {} != expected {}",
                    meta.len(),
                    file_size
                ));
            }
        }
        Ok(())
    }
}

/// Try to get file content length via HEAD, fallback to GET with range for Content-Range header
async fn get_content_length(url: &str) -> Result<u64, String> {
    let agent = || {
        ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(15))
            .timeout_read(std::time::Duration::from_secs(15))
            .build()
    };

    // Try HEAD first
    match agent().head(url).call() {
        Ok(response) => {
            if let Some(v) = response.header("Content-Length") {
                if let Ok(size) = v.parse::<u64>() {
                    return Ok(size);
                }
            }
            // Try Content-Range from HEAD
            if let Some(v) = response.header("Content-Range") {
                if let Some(size_str) = v.split('/').last() {
                    if let Ok(size) = size_str.parse::<u64>() {
                        return Ok(size);
                    }
                }
            }
        }
        Err(e) => {
            info!("HEAD request failed (will try GET range): {}", e);
        }
    }

    // Fallback: GET with Range: bytes=0-0 to get Content-Range header
    let url_owned = url.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let response = agent()
            .get(&url_owned)
            .set("Range", "bytes=0-0")
            .call()
            .map_err(|e| format!("Range probe request failed: {}", e))?;

        // Try Content-Range first (e.g. "bytes 0-0/12345678")
        if let Some(v) = response.header("Content-Range") {
            if let Some(size_str) = v.split('/').last() {
                if let Ok(size) = size_str.trim().parse::<u64>() {
                    return Ok(size);
                }
            }
        }
        // Fallback to Content-Length (might be just 1 byte from range)
        if let Some(v) = response.header("Content-Length") {
            if let Ok(size) = v.parse::<u64>() {
                // If server ignored range and sent full content
                return Ok(size);
            }
        }
        Err("Could not determine file size from server".to_string())
    })
    .await;

    match result {
        Ok(r) => r,
        Err(e) => Err(format!("Content-Length detection failed: {}", e)),
    }
}

/// Fetch DistributionInfo JSON from a URL
pub async fn fetch_distribution_json(url: &str) -> Result<String, String> {
    info!("Fetching distribution list from {}", url);

    let url_owned = url.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(15))
            .timeout_read(std::time::Duration::from_secs(30))
            .build();

        let response = agent
            .get(&url_owned)
            .call()
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        let text = response
            .into_string()
            .map_err(|e| format!("Failed to read response body: {}", e))?;

        Ok::<_, String>(text)
    })
    .await;

    match result {
        Ok(Ok(text)) => Ok(text),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("Task error: {}", e)),
    }
}

/// Get default URLs for distribution info
pub fn get_default_url(idx: usize) -> &'static str {
    match idx {
        0 => {
            "https://raw.giteeusercontent.com/mirrors/WSL/raw/master/distributions/DistributionInfo.json"
        }
        1 => {
            "https://raw.githubusercontent.com/microsoft/WSL/refs/heads/master/distributions/DistributionInfo.json"
        }
        _ => "",
    }
}
