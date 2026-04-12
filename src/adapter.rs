use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use zed_extension_api::serde_json::json;
use zed_extension_api::{
    serde_json, DebugAdapterBinary, DebugScenario, Os, StartDebuggingRequestArguments,
    StartDebuggingRequestArgumentsRequest, TaskTemplate, Worktree,
};

use crate::flutter::resolve_vm_service_info_file_path;

pub(crate) fn tool_binary(debug_mode: &str) -> &'static str {
    let (os, _) = zed_extension_api::current_platform();
    tool_binary_for_os(debug_mode, os)
}

pub(crate) fn tool_binary_for_os(debug_mode: &str, os: Os) -> &'static str {
    match (debug_mode, os) {
        ("flutter", Os::Windows) => "flutter.bat",
        ("flutter", _) => "flutter",
        (_, Os::Windows) => "dart.exe",
        (_, _) => "dart",
    }
}

pub(crate) fn preferred_windows_tool_path(
    default_name: &str,
    resolved_path: Option<String>,
) -> String {
    let Some(resolved_path) = resolved_path else {
        return default_name.to_string();
    };

    let resolved = PathBuf::from(&resolved_path);

    if default_name == "dart.exe" {
        let candidate = resolved
            .parent()
            .map(|parent| parent.join("dart.bat"))
            .filter(|path| path.exists());
        if let Some(candidate) = candidate {
            return candidate.to_string_lossy().to_string();
        }
    }

    resolved_path
}

pub(crate) fn tool_command_path(debug_mode: &str, worktree: &Worktree) -> String {
    let (os, _) = zed_extension_api::current_platform();
    let default_name = tool_binary_for_os(debug_mode, os);
    let resolved = worktree.which(tool_name(debug_mode));

    match os {
        Os::Windows => preferred_windows_tool_path(default_name, resolved),
        _ => resolved.unwrap_or_else(|| default_name.to_string()),
    }
}

pub(crate) fn tool_name(debug_mode: &str) -> &'static str {
    match debug_mode {
        "flutter" => "flutter",
        _ => "dart",
    }
}

pub(crate) fn ensure_flutter_device_tool_args(
    debug_mode: &str,
    request: &str,
    device_id: &str,
    tool_args: Vec<String>,
) -> Vec<String> {
    if debug_mode != "flutter" || request != "launch" {
        return tool_args;
    }

    let has_explicit_device = tool_args
        .iter()
        .any(|arg| arg == "-d" || arg == "--device-id" || arg.starts_with("--device-id="));

    if has_explicit_device {
        return tool_args;
    }

    let mut tool_args = tool_args;
    tool_args.extend(["-d".to_string(), device_id.to_string()]);
    tool_args
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn candidate_package_roots(program: &str, cwd: Option<&str>) -> Vec<PathBuf> {
    let program_path = Path::new(program);
    let mut roots = Vec::new();

    if let Some(cwd) = cwd {
        let cwd_path = PathBuf::from(cwd);
        if program_path.is_absolute() {
            if let Some(parent) = program_path.parent() {
                roots.push(parent.to_path_buf());
            }
        } else {
            roots.push(
                cwd_path
                    .join(program_path)
                    .parent()
                    .unwrap_or(&cwd_path)
                    .to_path_buf(),
            );
            roots.push(cwd_path);
        }
    } else if let Some(parent) = program_path.parent() {
        roots.push(parent.to_path_buf());
    }

    roots
}

fn package_uses_flutter(root: &Path) -> bool {
    for dir in root.ancestors() {
        let pubspec = dir.join("pubspec.yaml");
        if let Ok(contents) = fs::read_to_string(&pubspec) {
            if contents.contains("sdk: flutter")
                || contents.contains("flutter:")
                || contents.contains("package:flutter")
            {
                return true;
            }
        }

        let package_config = dir.join(".dart_tool").join("package_config.json");
        if let Ok(contents) = fs::read_to_string(&package_config) {
            if contents.contains("\"name\":\"flutter\"")
                || contents.contains("\"name\": \"flutter\"")
                || contents.contains("package:flutter")
            {
                return true;
            }
        }
    }

    false
}

pub(crate) fn infer_debug_mode(program: &str, cwd: Option<&str>) -> &'static str {
    if candidate_package_roots(program, cwd)
        .into_iter()
        .any(|root| package_uses_flutter(&root))
    {
        return "flutter";
    }

    let program = normalize_path(program);
    if program == "lib/main.dart" || program.starts_with("lib/") || program.contains("/lib/") {
        "flutter"
    } else {
        "dart"
    }
}

fn parse_string_array(config: &serde_json::Value, key: &str) -> Vec<String> {
    config
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect::<Vec<String>>()
        })
        .unwrap_or_default()
}

fn copy_optional_bool(config: &serde_json::Value, key: &str, target: &mut serde_json::Value) {
    if let Some(value) = config.get(key).and_then(|v| v.as_bool()) {
        target[key] = json!(value);
    }
}

fn parse_env(config: &serde_json::Value) -> Vec<(String, String)> {
    config
        .get("env")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                .collect::<Vec<(String, String)>>()
        })
        .unwrap_or_default()
}

pub(crate) fn dap_request_kind_from_config(
    config: &serde_json::Value,
) -> Result<StartDebuggingRequestArgumentsRequest, String> {
    match config.get("request") {
        Some(v) if v == "launch" => Ok(StartDebuggingRequestArgumentsRequest::Launch),
        Some(v) if v == "attach" => Ok(StartDebuggingRequestArgumentsRequest::Attach),
        Some(value) => Err(format!(
            "Unexpected value for `request` key in Dart debug adapter configuration: {value:?}"
        )),
        None => Err("Missing required `request` field in Dart debug adapter configuration".into()),
    }
}

pub(crate) fn build_dap_configuration(
    user_config: &serde_json::Value,
    worktree_root: &str,
) -> Result<(String, Vec<(String, String)>, Option<String>), String> {
    let debug_mode = user_config
        .get("type")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "type is required and cannot be empty or null".to_string())?;

    let program = user_config
        .get("program")
        .and_then(|v| v.as_str())
        .unwrap_or("lib/main.dart");

    let cwd = user_config
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| Some(worktree_root.to_string()));

    let request = user_config
        .get("request")
        .and_then(|v| v.as_str())
        .unwrap_or("launch");

    let vm_service_uri = user_config.get("vmServiceUri").and_then(|v| v.as_str());
    let explicit_vm_service_info_file = user_config
        .get("vmServiceInfoFile")
        .and_then(|v| v.as_str());

    if vm_service_uri.is_some() && explicit_vm_service_info_file.is_some() {
        return Err(
            "Provide only one of `vmServiceUri` or `vmServiceInfoFile` in Dart debug adapter configuration"
                .to_string(),
        );
    }

    let vm_service_info_file = resolve_vm_service_info_file_path(
        worktree_root,
        debug_mode,
        request,
        explicit_vm_service_info_file,
    )?;

    let args = parse_string_array(user_config, "args");
    let env = parse_env(user_config);

    let stop_on_entry = user_config
        .get("stopOnEntry")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let flutter_mode = user_config
        .get("flutterMode")
        .and_then(|v| v.as_str())
        .unwrap_or("debug");

    let device_id = user_config
        .get("device_id")
        .and_then(|v| v.as_str())
        .unwrap_or("chrome");

    let tool_args = ensure_flutter_device_tool_args(
        debug_mode,
        request,
        device_id,
        parse_string_array(user_config, "toolArgs"),
    );

    let platform = user_config
        .get("platform")
        .and_then(|v| v.as_str())
        .unwrap_or("web");

    let mut config_json = json!({
        "type": debug_mode,
        "request": request,
        "program": program,
        "cwd": cwd.clone().unwrap_or_default(),
        "args": args,
        "flutterMode": flutter_mode,
        "deviceId": device_id,
        "platform": platform,
        "stopOnEntry": stop_on_entry,
        "sendLogsToClient": true
    });

    if let Some(uri) = vm_service_uri {
        config_json["vmServiceUri"] = json!(uri);
    }

    if let Some(path) = vm_service_info_file {
        config_json["vmServiceInfoFile"] = json!(path);
    }

    if !tool_args.is_empty() {
        config_json["toolArgs"] = json!(tool_args);
    }

    let additional_project_paths = parse_string_array(user_config, "additionalProjectPaths");
    if !additional_project_paths.is_empty() {
        config_json["additionalProjectPaths"] = json!(additional_project_paths);
    }

    if let Some(custom_tool) = user_config.get("customTool").and_then(|v| v.as_str()) {
        config_json["customTool"] = json!(custom_tool);
    }

    if let Some(replaced_args) = user_config
        .get("customToolReplacesArgs")
        .and_then(|v| v.as_i64())
    {
        config_json["customToolReplacesArgs"] = json!(replaced_args);
    }

    copy_optional_bool(user_config, "noDebug", &mut config_json);
    copy_optional_bool(user_config, "debugSdkLibraries", &mut config_json);
    copy_optional_bool(
        user_config,
        "debugExternalPackageLibraries",
        &mut config_json,
    );
    copy_optional_bool(user_config, "evaluateGettersInDebugViews", &mut config_json);
    copy_optional_bool(
        user_config,
        "evaluateToStringInDebugViews",
        &mut config_json,
    );
    copy_optional_bool(user_config, "sendCustomProgressEvents", &mut config_json);

    Ok((config_json.to_string(), env, cwd))
}

pub(crate) fn build_debug_adapter_binary_from_config(
    user_config: &serde_json::Value,
    worktree_root: &str,
) -> Result<DebugAdapterBinary, String> {
    build_debug_adapter_binary_from_config_with_command(user_config, worktree_root, None)
}

pub(crate) fn build_debug_adapter_binary_from_config_with_worktree(
    user_config: &serde_json::Value,
    worktree_root: &str,
    worktree: &Worktree,
) -> Result<DebugAdapterBinary, String> {
    build_debug_adapter_binary_from_config_with_command(user_config, worktree_root, Some(worktree))
}

fn build_debug_adapter_binary_from_config_with_command(
    user_config: &serde_json::Value,
    worktree_root: &str,
    worktree: Option<&Worktree>,
) -> Result<DebugAdapterBinary, String> {
    let use_fvm = user_config
        .get("useFvm")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let debug_mode = user_config
        .get("type")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "type is required and cannot be empty or null".to_string())?;

    let (command, arguments) = if use_fvm {
        let fvm_tool = tool_name(debug_mode);
        (
            "fvm".to_string(),
            vec![fvm_tool.to_string(), "debug_adapter".to_string()],
        )
    } else {
        let command = if let Some(worktree) = worktree {
            tool_command_path(debug_mode, worktree)
        } else {
            tool_binary(debug_mode).to_string()
        };
        (command, vec!["debug_adapter".to_string()])
    };

    let request = user_config
        .get("request")
        .and_then(|v| v.as_str())
        .unwrap_or("launch");

    let (config_json, envs, cwd) = build_dap_configuration(user_config, worktree_root)?;

    Ok(DebugAdapterBinary {
        command: Some(command),
        arguments,
        envs,
        cwd,
        connection: None,
        request_args: StartDebuggingRequestArguments {
            configuration: config_json,
            request: match request {
                "attach" => StartDebuggingRequestArgumentsRequest::Attach,
                _ => StartDebuggingRequestArgumentsRequest::Launch,
            },
        },
    })
}

pub(crate) fn build_launch_config_value(
    debug_mode: &str,
    program: &str,
    cwd: Option<String>,
    args: Vec<String>,
    envs: Vec<(String, String)>,
    stop_on_entry: bool,
    tool_args: Vec<String>,
    flutter_mode: Option<&str>,
) -> serde_json::Value {
    let mut config_json = json!({
        "type": debug_mode,
        "request": "launch",
        "program": program,
        "cwd": cwd.unwrap_or_default(),
        "args": args,
        "stopOnEntry": stop_on_entry,
        "sendLogsToClient": true
    });

    if debug_mode == "flutter" {
        config_json["flutterMode"] = json!(flutter_mode.unwrap_or("debug"));
        config_json["deviceId"] = json!("chrome");
        config_json["platform"] = json!("web");
    }

    if !tool_args.is_empty() {
        config_json["toolArgs"] = json!(tool_args);
    }

    if !envs.is_empty() {
        config_json["env"] = json!(envs
            .into_iter()
            .collect::<std::collections::BTreeMap<String, String>>());
    }

    config_json
}

fn debug_label_from_resolved_label(resolved_label: &str) -> String {
    if let Some(rest) = resolved_label.strip_prefix("Run ") {
        return format!("Debug {rest}");
    }
    if let Some(rest) = resolved_label.strip_prefix("Run") {
        return format!("Debug{rest}");
    }
    resolved_label.to_string()
}

fn flutter_mode_from_tool_args(tool_args: &[String]) -> (&'static str, Vec<String>) {
    let mut filtered = Vec::with_capacity(tool_args.len());
    let mut flutter_mode = "debug";

    for arg in tool_args {
        match arg.as_str() {
            "--profile" => flutter_mode = "profile",
            "--release" => flutter_mode = "release",
            _ => filtered.push(arg.clone()),
        }
    }

    (flutter_mode, filtered)
}

pub(crate) fn build_debug_scenario_from_task(
    task: &TaskTemplate,
    resolved_label: String,
    debug_adapter_name: String,
) -> Option<DebugScenario> {
    let command = task.command.as_str();
    let args = task.args.as_slice();

    match (command, args) {
        ("flutter", [run, rest @ ..]) if run == "run" => {
            let (flutter_mode, tool_args) = flutter_mode_from_tool_args(rest);
            let label = if flutter_mode == "debug" {
                debug_label_from_resolved_label(&resolved_label)
            } else {
                resolved_label
            };

            Some(DebugScenario {
                adapter: debug_adapter_name,
                label,
                config: build_launch_config_value(
                    "flutter",
                    "lib/main.dart",
                    task.cwd.clone(),
                    Vec::new(),
                    task.env.clone(),
                    false,
                    ensure_flutter_device_tool_args("flutter", "launch", "chrome", tool_args),
                    Some(flutter_mode),
                )
                .to_string(),
                tcp_connection: None,
                build: None,
            })
        }
        ("fvm", [tool, run, rest @ ..]) if tool == "flutter" && run == "run" => {
            let (flutter_mode, tool_args) = flutter_mode_from_tool_args(rest);
            let label = if flutter_mode == "debug" {
                debug_label_from_resolved_label(&resolved_label)
            } else {
                resolved_label
            };
            let mut config = build_launch_config_value(
                "flutter",
                "lib/main.dart",
                task.cwd.clone(),
                Vec::new(),
                task.env.clone(),
                false,
                ensure_flutter_device_tool_args("flutter", "launch", "chrome", tool_args),
                Some(flutter_mode),
            );
            config["useFvm"] = json!(true);

            Some(DebugScenario {
                adapter: debug_adapter_name,
                label,
                config: config.to_string(),
                tcp_connection: None,
                build: None,
            })
        }
        ("dart", [run, program, rest @ ..]) if run == "run" => Some(DebugScenario {
            adapter: debug_adapter_name,
            label: debug_label_from_resolved_label(&resolved_label),
            config: build_launch_config_value(
                infer_debug_mode(program, task.cwd.as_deref()),
                program,
                task.cwd.clone(),
                Vec::new(),
                task.env.clone(),
                false,
                rest.to_vec(),
                None,
            )
            .to_string(),
            tcp_connection: None,
            build: None,
        }),
        ("fvm", [tool, run, program, rest @ ..]) if tool == "dart" && run == "run" => {
            let mut config = build_launch_config_value(
                infer_debug_mode(program, task.cwd.as_deref()),
                program,
                task.cwd.clone(),
                Vec::new(),
                task.env.clone(),
                false,
                rest.to_vec(),
                None,
            );
            config["useFvm"] = json!(true);

            Some(DebugScenario {
                adapter: debug_adapter_name,
                label: debug_label_from_resolved_label(&resolved_label),
                config: config.to_string(),
                tcp_connection: None,
                build: None,
            })
        }
        _ => None,
    }
}

#[allow(dead_code)]
fn _touch_fs(_path: &Path) {
    let _ = fs::metadata(_path);
    let _ = SystemTime::now().duration_since(UNIX_EPOCH);
}
