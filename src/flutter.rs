use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use zed_extension_api::{serde_json, Result};

pub(crate) const VM_SERVICE_INFO_DIR: &str = ".zed/dart/vmservice-info";

fn vm_service_info_dir(worktree_root: &str) -> PathBuf {
    Path::new(worktree_root).join(VM_SERVICE_INFO_DIR)
}

pub(crate) fn resolve_vm_service_info_file_path(
    worktree_root: &str,
    debug_mode: &str,
    request: &str,
    explicit_vm_service_info_file: Option<&str>,
) -> Result<Option<String>, String> {
    if let Some(path) = explicit_vm_service_info_file {
        return Ok(Some(path.to_string()));
    }

    if debug_mode != "flutter" || request != "launch" {
        return Ok(None);
    }

    let directory = vm_service_info_dir(worktree_root);
    fs::create_dir_all(&directory).map_err(|err| {
        format!(
            "Failed to create VM service info directory `{}`: {err}",
            directory.display()
        )
    })?;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| format!("Failed to determine current time: {err}"))?
        .as_nanos();
    let file_path = directory.join(format!(
        "flutter-vmservice-{}-{}.json",
        std::process::id(),
        timestamp
    ));

    Ok(Some(file_path.to_string_lossy().to_string()))
}

pub(crate) fn read_vm_service_uri_from_info_file(path: &Path) -> Result<String, String> {
    let contents = fs::read_to_string(path).map_err(|err| {
        format!(
            "Failed to read VM service info file `{}`: {err}",
            path.display()
        )
    })?;
    let value: serde_json::Value = serde_json::from_str(&contents).map_err(|err| {
        format!(
            "Invalid VM service info JSON in `{}`: {err}",
            path.display()
        )
    })?;

    for key in ["uri", "vmServiceUri", "wsUri", "ws_uri"] {
        if let Some(uri) = value.get(key).and_then(|v| v.as_str()) {
            return normalize_vm_service_uri(uri);
        }
    }

    Err(format!(
        "VM service info file `{}` does not contain a supported URI field",
        path.display()
    ))
}

pub(crate) fn discover_latest_vm_service_info_file(worktree_root: &str) -> Result<PathBuf, String> {
    let directory = vm_service_info_dir(worktree_root);
    let entries = fs::read_dir(&directory).map_err(|err| {
        format!(
            "Failed to read VM service info directory `{}`: {err}",
            directory.display()
        )
    })?;

    let mut files = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect::<Vec<PathBuf>>();

    if files.is_empty() {
        return Err(format!(
            "No VM service info files found in `{}`. Start a Flutter debug launch first or provide a VM service URI.",
            directory.display()
        ));
    }

    files.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(UNIX_EPOCH)
    });
    files.reverse();

    files
        .into_iter()
        .next()
        .ok_or_else(|| "Failed to select a VM service info file".to_string())
}

pub(crate) fn normalize_vm_service_uri(vm_service_uri: &str) -> Result<String, String> {
    let trimmed = vm_service_uri.trim();
    if trimmed.is_empty() {
        return Err("VM service URI is required".to_string());
    }

    if trimmed.starts_with("ws://")
        || trimmed.starts_with("wss://")
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
    {
        return Ok(trimmed.to_string());
    }

    Ok(format!("http://{trimmed}"))
}

pub(crate) fn vm_service_websocket_uri(vm_service_uri: &str) -> Result<String, String> {
    let normalized = normalize_vm_service_uri(vm_service_uri)?;

    if let Some(uri) = normalized.strip_prefix("ws://") {
        return Ok(format!("ws://{uri}"));
    }

    if let Some(uri) = normalized.strip_prefix("wss://") {
        return Ok(format!("wss://{uri}"));
    }

    if let Some(uri) = normalized.strip_prefix("http://") {
        return Ok(format!("ws://{}", ensure_vm_service_ws_path(uri)));
    }

    if let Some(uri) = normalized.strip_prefix("https://") {
        return Ok(format!("wss://{}", ensure_vm_service_ws_path(uri)));
    }

    Err(format!(
        "Unsupported VM service URI `{normalized}`. Expected ws://, wss://, http://, or https://"
    ))
}

fn ensure_vm_service_ws_path(uri_without_scheme: &str) -> String {
    if uri_without_scheme.ends_with("/ws") || uri_without_scheme.ends_with("/ws/") {
        return uri_without_scheme.trim_end_matches('/').to_string();
    }

    if uri_without_scheme.ends_with('/') {
        return format!("{uri_without_scheme}ws");
    }

    if uri_without_scheme.contains('/') {
        return uri_without_scheme.to_string();
    }

    format!("{uri_without_scheme}/ws")
}
