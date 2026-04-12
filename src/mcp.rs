use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use zed_extension_api::process::Command as ProcessCommand;
use zed_extension_api::{self as zed, serde_json::json, ContextServerConfiguration, Project};

pub(crate) const FLUTTER_CONTEXT_SERVER_ID: &str = "flutter-tools";

fn is_flutter_context_server(context_server_id: &zed::ContextServerId) -> bool {
    context_server_id.as_ref() == FLUTTER_CONTEXT_SERVER_ID
}

fn extension_root() -> Result<PathBuf, String> {
    std::env::var("PWD")
        .map(PathBuf::from)
        .map_err(|err| format!("Failed to resolve extension root from PWD: {err}"))
}

fn installed_extension_root(extension_root: &Path) -> PathBuf {
    let parent = extension_root.parent();
    let grandparent = parent.and_then(Path::parent);

    if parent.and_then(Path::file_name) == Some(OsStr::new("work")) {
        if let Some(extensions_root) = grandparent {
            return extensions_root.join("installed").join("dart");
        }
    }

    extension_root.to_path_buf()
}

fn cache_root(extension_root: &Path) -> PathBuf {
    let parent = extension_root.parent();
    let grandparent = parent.and_then(Path::parent);

    if parent.and_then(Path::file_name) == Some(OsStr::new("installed"))
        || parent.and_then(Path::file_name) == Some(OsStr::new("work"))
    {
        if let Some(extensions_root) = grandparent {
            return extensions_root.join("work").join("dart").join("mcp");
        }
    }

    extension_root.join(".zed-dart").join("mcp")
}

fn cache_env_root(extension_root: &Path) -> PathBuf {
    cache_root(extension_root).join("dart-env")
}

fn dart_command_name() -> &'static str {
    let (os, _) = zed::current_platform();
    match os {
        zed::Os::Windows => "dart.exe",
        _ => "dart",
    }
}

fn compiled_server_name() -> &'static str {
    let (os, _) = zed::current_platform();
    match os {
        zed::Os::Windows => "flutter_tools_mcp.exe",
        _ => "flutter_tools_mcp",
    }
}

fn source_entrypoint(extension_root: &Path) -> PathBuf {
    installed_extension_root(extension_root)
        .join("tool")
        .join("mcp_server.dart")
}

fn source_paths(extension_root: &Path) -> Vec<PathBuf> {
    let source_root = installed_extension_root(extension_root);
    vec![
        source_root.join("tool").join("mcp_server.dart"),
        source_root.join("tool").join("src").join("mcp"),
    ]
}

fn modified_time(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|metadata| metadata.modified()).ok()
}

fn newest_source_mtime(path: &Path) -> Option<SystemTime> {
    if path.is_file() {
        return modified_time(path);
    }

    let entries = fs::read_dir(path).ok()?;
    let mut newest: Option<SystemTime> = None;
    for entry in entries.filter_map(|entry| entry.ok()) {
        let candidate = newest_source_mtime(&entry.path());
        newest = match (newest, candidate) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (None, Some(right)) => Some(right),
            (existing, None) => existing,
        };
    }
    newest
}

fn needs_recompile(output_path: &Path, extension_root: &Path) -> bool {
    let Some(output_mtime) = modified_time(output_path) else {
        return true;
    };

    source_paths(extension_root)
        .into_iter()
        .filter_map(|path| newest_source_mtime(&path))
        .any(|source_mtime| source_mtime > output_mtime)
}

fn ensure_compiled_server(extension_root: &Path) -> Result<PathBuf, String> {
    let cache_root = cache_root(extension_root);
    fs::create_dir_all(&cache_root).map_err(|err| {
        format!(
            "Failed to create MCP cache directory `{}`: {err}",
            cache_root.display()
        )
    })?;
    let env_root = cache_env_root(extension_root);
    fs::create_dir_all(&env_root).map_err(|err| {
        format!(
            "Failed to create Dart MCP environment directory `{}`: {err}",
            env_root.display()
        )
    })?;

    let output_path = cache_root.join(compiled_server_name());
    if !needs_recompile(&output_path, extension_root) {
        return Ok(output_path);
    }

    let source = source_entrypoint(extension_root);
    let output = ProcessCommand::new(dart_command_name())
        .arg("--disable-analytics")
        .arg("--suppress-analytics")
        .arg("compile")
        .arg("exe")
        .arg(source.to_string_lossy().to_string())
        .arg("-o")
        .arg(output_path.to_string_lossy().to_string())
        .env("APPDATA", env_root.to_string_lossy().to_string())
        .env("LOCALAPPDATA", env_root.to_string_lossy().to_string())
        .env("HOME", env_root.to_string_lossy().to_string())
        .output()?;

    if output.status != Some(0) {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(if detail.is_empty() {
            "Failed to compile bundled Flutter MCP server".to_string()
        } else {
            format!("Failed to compile bundled Flutter MCP server: {detail}")
        });
    }

    if !output_path.is_file() {
        return Err(format!(
            "Compiled Flutter MCP server was not created at `{}`",
            output_path.display()
        ));
    }

    Ok(output_path)
}

pub(crate) fn context_server_command(
    context_server_id: &zed::ContextServerId,
    _project: &Project,
) -> Result<zed::Command, String> {
    if !is_flutter_context_server(context_server_id) {
        return Err(format!(
            "Unknown context server `{}`",
            context_server_id.as_ref()
        ));
    }

    let extension_root = extension_root()?;
    let executable = ensure_compiled_server(&extension_root)?;

    Ok(zed::Command {
        command: executable.to_string_lossy().to_string(),
        args: Vec::new(),
        env: Vec::new(),
    })
}

pub(crate) fn context_server_configuration(
    context_server_id: &zed::ContextServerId,
    _project: &Project,
) -> Result<Option<ContextServerConfiguration>, String> {
    if !is_flutter_context_server(context_server_id) {
        return Ok(None);
    }

    Ok(Some(ContextServerConfiguration {
        installation_instructions: [
            "This server is bundled with the Dart extension.",
            "",
            "Prerequisites:",
            "- `dart` must be available on your PATH the first time Zed compiles the bundled MCP server into a cached executable.",
            "- `flutter` should be available on your PATH for the Flutter device and hot-reload tools.",
            "",
            "Implementation details:",
            "- The bundled server uses only `dart:` libraries and relative imports so compilation does not require `pub get` or pub.dev access.",
            "- The compiled executable is cached under your system temp directory and rebuilt when the bundled MCP sources change.",
            "",
            "Notes:",
            "- `flutter_hot_reload` and `flutter_hot_restart` accept an explicit `workspace_root` when you want them to auto-discover the latest `.zed/dart/vmservice-info` file.",
            "- `flutter_hot_reload` and `flutter_hot_restart` currently use the same shell-backed `flutter attach` workaround as the slash commands.",
        ]
        .join("\n"),
        settings_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {}
        })
        .to_string(),
        default_settings: json!({}).to_string(),
    }))
}
