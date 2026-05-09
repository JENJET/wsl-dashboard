use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use tracing::{info, error, debug};

/// VHDX metadata parsed directly from file headers
#[derive(Debug, Clone, Default)]
pub struct VhdxInfo {
    pub virtual_size: String,
    pub vhd_type: String,
    pub is_sparse: bool,
}

// VHDX GUID constants
const METADATA_REGION_GUID: [u8; 16] = [
    0x06, 0xA2, 0x7C, 0x8B, 0x90, 0x47, 0x9A, 0x4B,
    0xB8, 0xFE, 0x57, 0x5F, 0x05, 0x0F, 0x88, 0x6E,
];
// {2DC27766-F623-4200-9D64-115E9BFD4A08}
const BAT_REGION_GUID: [u8; 16] = [
    0x66, 0x77, 0xC2, 0x2D, 0x23, 0xF6, 0x00, 0x42,
    0x9D, 0x64, 0x11, 0x5E, 0x9B, 0xFD, 0x4A, 0x08,
];
const VIRTUAL_DISK_SIZE_GUID: [u8; 16] = [
    0x24, 0x42, 0xA5, 0x2F, 0x1B, 0xCD, 0x76, 0x48,
    0xB2, 0x11, 0x5D, 0xBE, 0xD8, 0x3B, 0xF4, 0xB8,
];

const VHDX_HEADER_SIZE: u64 = 65536; // 64KB

/// Parse VHDX metadata by reading the file headers directly.
/// No admin privileges needed — just file read access.
pub fn get_vhdx_info(vhdx_path: &str) -> Option<VhdxInfo> {
    let path = Path::new(vhdx_path);
    if !path.exists() {
        debug!("VHDX file not found: {}", vhdx_path);
        return None;
    }

    let mut file = File::open(path).ok()?;
    let file_len = file.metadata().ok()?.len();

    // Must be at least 64KB + 4KB (header1) + 4KB (header2) + region table
    if file_len < VHDX_HEADER_SIZE * 4 {
        debug!("File too small to be VHDX: {} bytes", file_len);
        return None;
    }

    // Read file identifier (first 8 bytes)
    let mut sig = [0u8; 8];
    file.read_exact(&mut sig).ok()?;
    if &sig != b"vhdxfile" {
        // Check if it's a legacy VHD file
        if &sig[..7] == b"conecti" {
            return parse_vhd_info(&mut file, file_len);
        }
        debug!("Not a VHDX file: invalid signature");
        return None;
    }

    // Read and compare two headers at 64KB and 128KB
    let header1 = read_exact_at(&mut file, VHDX_HEADER_SIZE, 4096)?;
    let header2 = read_exact_at(&mut file, VHDX_HEADER_SIZE * 2, 4096)?;

    if &header1[0..4] != b"head" || &header2[0..4] != b"head" {
        debug!("VHDX: invalid header signatures");
        return None;
    }

    let seq1 = u64::from_le_bytes(header1[8..16].try_into().ok()?);
    let seq2 = u64::from_le_bytes(header2[8..16].try_into().ok()?);

    // Active region table follows the active header:
    // Region Table 1 at offset 192KB (after header1), Region Table 2 at 256KB
    let region_offset = if seq1 >= seq2 {
        VHDX_HEADER_SIZE * 3
    } else {
        VHDX_HEADER_SIZE * 4
    };

    // Read region table
    let region_header = read_exact_at(&mut file, region_offset, 16)?;
    if &region_header[0..4] != b"regi" {
        debug!("VHDX: invalid region table signature");
        return None;
    }
    let entry_count = u32::from_le_bytes(region_header[8..12].try_into().ok()?) as usize;

    let mut metadata_offset = None;
    let mut has_bat = false;

    for i in 0..entry_count {
        let entry = read_exact_at(&mut file, region_offset + 16 + (i as u64) * 32, 32)?;
        let guid = &entry[0..16];
        let offset = u64::from_le_bytes(entry[16..24].try_into().ok()?);

        if guid == METADATA_REGION_GUID {
            metadata_offset = Some(offset);
        }
        if guid == BAT_REGION_GUID {
            has_bat = true;
        }
    }

    let metadata_off = metadata_offset?;

    // Read metadata region
    let meta_header = read_exact_at(&mut file, metadata_off, 32)?;
    if &meta_header[0..8] != b"metadata" {
        debug!("VHDX: invalid metadata signature");
        return None;
    }
    let meta_entry_count = u16::from_le_bytes(meta_header[10..12].try_into().ok()?) as usize;

    let mut virtual_size: u64 = 0;

    for i in 0..meta_entry_count {
        let entry = read_exact_at(&mut file, metadata_off + 32 + (i as u64) * 32, 32)?;
        let item_guid = &entry[0..16];

        if item_guid == VIRTUAL_DISK_SIZE_GUID {
            let item_offset = u32::from_le_bytes(entry[16..20].try_into().ok()?) as u64;
            let item_length = u32::from_le_bytes(entry[20..24].try_into().ok()?) as usize;
            let abs_offset = metadata_off + item_offset;

            let mut size_bytes = [0u8; 8];
            let read_len = item_length.min(8);
            file.seek(SeekFrom::Start(abs_offset)).ok()?;
            file.read_exact(&mut size_bytes[..read_len]).ok()?;
            virtual_size = u64::from_le_bytes(size_bytes);
            break;
        }
    }

    let info = VhdxInfo {
        virtual_size: format_size(virtual_size),
        vhd_type: if has_bat {
            "VHDX (Dynamic)".to_string()
        } else {
            "VHDX (Fixed)".to_string()
        },
        is_sparse: has_bat,
    };
    Some(info)
}

/// Parse legacy VHD format (conectix signature)
fn parse_vhd_info(file: &mut File, _file_len: u64) -> Option<VhdxInfo> {
    // VHD footer is at the end of the file (512 bytes)
    // We need to read the footer to get disk size and type
    let footer_offset = _file_len.saturating_sub(512);
    let footer = read_exact_at(file, footer_offset, 512)?;

    if &footer[0..8] != b"conectix" {
        return None;
    }

    // Disk type at offset 0x3C (4 bytes): 2 = Fixed, 3 = Dynamic, 4 = Differencing
    let disk_type = u32::from_be_bytes(footer[0x3C..0x40].try_into().ok()?);
    // Current size at offset 0x28 (8 bytes, big-endian)
    let current_size = u64::from_be_bytes(footer[0x28..0x30].try_into().ok()?);

    let vhd_type = match disk_type {
        2 => "VHD (Fixed)".to_string(),
        3 => "VHD (Dynamic)".to_string(),
        4 => "VHD (Differencing)".to_string(),
        _ => format!("VHD (Type {})", disk_type),
    };

    Some(VhdxInfo {
        virtual_size: format_size(current_size),
        vhd_type,
        is_sparse: disk_type == 3,
    })
}

fn read_exact_at(file: &mut File, offset: u64, len: usize) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = vec![0u8; len];
    file.read_exact(&mut buf).ok()?;
    Some(buf)
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 * 1024 {
        format!("{:.2} TB", bytes as f64 / (1024.0 * 1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Resize a VHDX disk using elevated PowerShell.
/// This requires admin privileges and will show a UAC prompt.
pub fn resize_vhdx(vhdx_path: &str, new_size_bytes: u64) -> Result<(), String> {
    let path = Path::new(vhdx_path);
    if !path.exists() {
        return Err(format!("VHDX file not found: {}", vhdx_path));
    }

    info!("Resizing VHDX {} to {} bytes", vhdx_path, new_size_bytes);

    let size_str = new_size_bytes.to_string();
    // Write a temp script that does the resize and writes result to a temp file
    let temp_dir = std::env::temp_dir();
    let script_path = temp_dir.join("wsldashboard_resize_vhdx.ps1");
    let result_path = temp_dir.join("wsldashboard_resize_result.txt");

    // Clean up any previous result file
    let _ = std::fs::remove_file(&result_path);

    let script = format!(
        r#"$ErrorActionPreference = "Stop"
try {{
    Resize-VHD -Path '{path}' -SizeBytes {size} -ErrorAction Stop
    "SUCCESS:{size}" | Out-File -FilePath '{result}' -Encoding UTF8
}} catch {{
    "ERROR:$($_.Exception.Message)" | Out-File -FilePath '{result}' -Encoding UTF8
}}"#,
        path = vhdx_path.replace('\'', "''"),
        size = size_str,
        result = result_path.to_string_lossy().replace('\'', "''"),
    );

    std::fs::write(&script_path, &script)
        .map_err(|e| format!("Failed to write temp script: {}", e))?;

    // Execute script with elevation via Start-Process -Verb RunAs, hidden window
    let ps_command = format!(
        "Start-Process powershell.exe -ArgumentList '-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File \"{}\"' -Verb RunAs -Wait -WindowStyle Hidden",
        script_path.to_string_lossy()
    );

    info!("Launching elevated PowerShell for VHDX resize...");
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps_command])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to run PowerShell: {}", e))?;

    // Clean up script
    let _ = std::fs::remove_file(&script_path);

    // Read result file
    if let Ok(result_str) = std::fs::read_to_string(&result_path) {
        let _ = std::fs::remove_file(&result_path);
        if result_str.starts_with("SUCCESS:") {
            info!("VHDX resize completed successfully: {}", vhdx_path);
            Ok(())
        } else {
            let err = result_str
                .strip_prefix("ERROR:")
                .unwrap_or(&result_str)
                .trim()
                .to_string();
            error!("VHDX resize failed: {}", err);
            Err(format!("Resize failed: {}", err))
        }
    } else {
        // Maybe the user cancelled the UAC prompt, or something else went wrong
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!("VHDX resize: no result file. stderr: {}", stderr);
        let _ = std::fs::remove_file(&result_path);
        Err("操作被取消或需要管理员权限".to_string())
    }
}
