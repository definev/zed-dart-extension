use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use zed_extension_api::serde_json::{self, json};
use zed_extension_api::{
    DebugConfig, DebugRequest, Extension, StartDebuggingRequestArgumentsRequest, TaskTemplate,
};

use crate::adapter::{
    build_dap_configuration, build_debug_adapter_binary_from_config,
    build_debug_scenario_from_task, dap_request_kind_from_config, infer_debug_mode,
    preferred_windows_tool_path,
};
use crate::device::FlutterDevice;
use crate::device::{
    filter_flutter_devices, format_flutter_devices_output, parse_flutter_devices_json,
};
use crate::flutter::{
    normalize_vm_service_uri, read_vm_service_uri_from_info_file,
    resolve_vm_service_info_file_path, vm_service_websocket_uri,
};
use crate::slash::{
    build_devtools_url, build_flutter_attach_shell_script,
    resolve_flutter_hot_command_target_for_root,
};
use crate::DartExtension;

#[test]
fn request_kind_supports_launch_and_attach() {
    let launch = json!({ "request": "launch" });
    let attach = json!({ "request": "attach" });

    assert!(matches!(
        dap_request_kind_from_config(&launch),
        Ok(StartDebuggingRequestArgumentsRequest::Launch)
    ));
    assert!(matches!(
        dap_request_kind_from_config(&attach),
        Ok(StartDebuggingRequestArgumentsRequest::Attach)
    ));
}

#[test]
fn request_kind_rejects_missing_or_invalid_values() {
    let missing = json!({});
    let invalid = json!({ "request": "weird" });

    assert!(dap_request_kind_from_config(&missing).is_err());
    assert!(dap_request_kind_from_config(&invalid).is_err());
}

#[test]
fn build_dap_configuration_uses_debug_mode_and_optional_fields() {
    let user_config = json!({
        "type": "flutter",
        "request": "attach",
        "program": "lib/main.dart",
        "cwd": "example",
        "args": ["--flavor=dev"],
        "env": {
            "FOO": "bar"
        },
        "flutterMode": "profile",
        "device_id": "macos",
        "platform": "desktop",
        "stopOnEntry": true,
        "vmServiceUri": "ws://127.0.0.1:8181/ws",
        "customTool": "wrapper",
        "customToolReplacesArgs": 1,
        "additionalProjectPaths": ["../shared", "../packages"],
        "noDebug": false,
        "debugSdkLibraries": true,
        "debugExternalPackageLibraries": false,
        "evaluateGettersInDebugViews": true,
        "evaluateToStringInDebugViews": true,
        "sendCustomProgressEvents": true
    });

    let (config_json, env, cwd) =
        build_dap_configuration(&user_config, "workspace").expect("valid config");
    let parsed: serde_json::Value =
        serde_json::from_str(&config_json).expect("valid generated json");

    assert_eq!(parsed["type"], "flutter");
    assert_eq!(parsed["request"], "attach");
    assert_eq!(parsed["program"], "lib/main.dart");
    assert_eq!(parsed["cwd"], "example");
    assert_eq!(parsed["args"], json!(["--flavor=dev"]));
    assert_eq!(parsed["flutterMode"], "profile");
    assert_eq!(parsed["deviceId"], "macos");
    assert_eq!(parsed["platform"], "desktop");
    assert_eq!(parsed["stopOnEntry"], true);
    assert_eq!(parsed["vmServiceUri"], "ws://127.0.0.1:8181/ws");
    assert_eq!(parsed["customTool"], "wrapper");
    assert_eq!(parsed["customToolReplacesArgs"], 1);
    assert_eq!(
        parsed["additionalProjectPaths"],
        json!(["../shared", "../packages"])
    );
    assert_eq!(parsed["noDebug"], false);
    assert_eq!(parsed["debugSdkLibraries"], true);
    assert_eq!(parsed["debugExternalPackageLibraries"], false);
    assert_eq!(parsed["evaluateGettersInDebugViews"], true);
    assert_eq!(parsed["evaluateToStringInDebugViews"], true);
    assert_eq!(parsed["sendCustomProgressEvents"], true);
    assert_eq!(parsed["sendLogsToClient"], true);
    assert!(parsed.get("toolArgs").is_none());
    assert_eq!(env, vec![("FOO".to_string(), "bar".to_string())]);
    assert_eq!(cwd, Some("example".to_string()));
}

#[test]
fn build_dap_configuration_supports_vm_service_info_file_for_attach() {
    let user_config = json!({
        "type": "flutter",
        "request": "attach",
        "vmServiceInfoFile": ".dart_tool/vm.json"
    });

    let (config_json, _, _) =
        build_dap_configuration(&user_config, "workspace").expect("valid config");
    let parsed: serde_json::Value =
        serde_json::from_str(&config_json).expect("valid generated json");

    assert_eq!(parsed["vmServiceInfoFile"], ".dart_tool/vm.json");
    assert!(parsed.get("vmServiceUri").is_none());
}

#[test]
fn build_dap_configuration_does_not_add_vm_service_info_file_for_dart_launches() {
    let user_config = json!({
        "type": "dart",
        "request": "launch",
        "program": "bin/main.dart"
    });

    let (config_json, _, _) =
        build_dap_configuration(&user_config, "workspace").expect("valid config");
    let parsed: serde_json::Value =
        serde_json::from_str(&config_json).expect("valid generated json");

    assert!(parsed.get("vmServiceInfoFile").is_none());
}

#[test]
fn build_dap_configuration_does_not_add_vm_service_info_file_for_flutter_attach() {
    let user_config = json!({
        "type": "flutter",
        "request": "attach",
        "program": "lib/main.dart"
    });

    let (config_json, _, _) =
        build_dap_configuration(&user_config, "workspace").expect("valid config");
    let parsed: serde_json::Value =
        serde_json::from_str(&config_json).expect("valid generated json");

    assert!(parsed.get("vmServiceInfoFile").is_none());
}

#[test]
fn build_dap_configuration_rejects_both_vm_service_uri_and_info_file() {
    let user_config = json!({
        "type": "flutter",
        "request": "attach",
        "vmServiceUri": "ws://127.0.0.1:8181/ws",
        "vmServiceInfoFile": ".dart_tool/vm.json"
    });

    assert!(build_dap_configuration(&user_config, "workspace").is_err());
}

#[test]
fn build_dap_configuration_omits_optional_fields_when_absent() {
    let user_config = json!({
        "type": "dart"
    });

    let (config_json, env, cwd) =
        build_dap_configuration(&user_config, "workspace").expect("valid config");
    let parsed: serde_json::Value =
        serde_json::from_str(&config_json).expect("valid generated json");

    assert_eq!(parsed["type"], "dart");
    assert_eq!(parsed["request"], "launch");
    assert_eq!(parsed["program"], "lib/main.dart");
    assert_eq!(parsed["cwd"], "workspace");
    assert_eq!(parsed["flutterMode"], "debug");
    assert_eq!(parsed["deviceId"], "chrome");
    assert_eq!(parsed["platform"], "web");
    assert_eq!(parsed["stopOnEntry"], false);
    assert!(parsed.get("toolArgs").is_none());
    assert!(parsed.get("vmServiceUri").is_none());
    assert!(env.is_empty());
    assert_eq!(cwd, Some("workspace".to_string()));
}

#[test]
fn build_dap_configuration_adds_flutter_device_tool_args_for_launch() {
    let user_config = json!({
        "type": "flutter",
        "request": "launch",
        "device_id": "android"
    });

    let (config_json, _, _) =
        build_dap_configuration(&user_config, "workspace").expect("valid config");
    let parsed: serde_json::Value =
        serde_json::from_str(&config_json).expect("valid generated json");

    assert_eq!(parsed["toolArgs"], json!(["-d", "android"]));
    assert!(parsed["vmServiceInfoFile"].as_str().is_some());
}

#[test]
fn build_dap_configuration_does_not_override_explicit_device_tool_args() {
    let user_config = json!({
        "type": "flutter",
        "request": "launch",
        "device_id": "android",
        "toolArgs": ["--device-id=macos"]
    });

    let (config_json, _, _) =
        build_dap_configuration(&user_config, "workspace").expect("valid config");
    let parsed: serde_json::Value =
        serde_json::from_str(&config_json).expect("valid generated json");

    assert_eq!(parsed["toolArgs"], json!(["--device-id=macos"]));
}

#[test]
fn build_dap_configuration_does_not_add_device_tool_args_for_attach() {
    let user_config = json!({
        "type": "flutter",
        "request": "attach",
        "device_id": "android"
    });

    let (config_json, _, _) =
        build_dap_configuration(&user_config, "workspace").expect("valid config");
    let parsed: serde_json::Value =
        serde_json::from_str(&config_json).expect("valid generated json");

    assert!(parsed.get("toolArgs").is_none());
}

#[test]
fn infer_debug_mode_distinguishes_common_flutter_and_dart_entrypoints() {
    assert_eq!(infer_debug_mode("lib/main.dart", None), "flutter");
    assert_eq!(
        infer_debug_mode("packages/app/lib/main.dart", None),
        "flutter"
    );
    assert_eq!(infer_debug_mode("bin/main.dart", None), "dart");
}

#[test]
fn infer_debug_mode_uses_nearest_package_context_when_available() {
    let root = std::env::temp_dir().join(format!(
        "zed-dart-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    let package_root = root.join("packages").join("app");
    fs::create_dir_all(package_root.join("lib")).expect("package root");
    fs::write(
        package_root.join("pubspec.yaml"),
        "name: app\ndependencies:\n  flutter:\n    sdk: flutter\n",
    )
    .expect("pubspec");

    assert_eq!(
        infer_debug_mode("lib/main.dart", Some(&package_root.to_string_lossy())),
        "flutter"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn dap_config_to_scenario_infers_flutter_launches() {
    let mut extension = DartExtension;
    let scenario = extension
        .dap_config_to_scenario(DebugConfig {
            label: "Run App".to_string(),
            adapter: "Dart".to_string(),
            request: DebugRequest::Launch(zed_extension_api::LaunchRequest {
                program: "lib/main.dart".to_string(),
                cwd: Some("app".to_string()),
                args: vec!["--flavor=dev".to_string()],
                envs: vec![("FOO".to_string(), "bar".to_string())],
            }),
            stop_on_entry: Some(true),
        })
        .expect("scenario");

    let parsed: serde_json::Value =
        serde_json::from_str(&scenario.config).expect("valid generated json");

    assert_eq!(scenario.adapter, "Dart");
    assert_eq!(parsed["type"], "flutter");
    assert_eq!(parsed["program"], "lib/main.dart");
    assert_eq!(parsed["cwd"], "app");
    assert_eq!(parsed["args"], json!(["--flavor=dev"]));
    assert_eq!(parsed["env"], json!({ "FOO": "bar" }));
    assert_eq!(parsed["stopOnEntry"], true);
}

#[test]
fn locator_creates_flutter_scenario_from_flutter_run_task() {
    let task = TaskTemplate {
        label: "Run App".to_string(),
        command: "flutter".to_string(),
        args: vec!["run".to_string(), "--flavor".to_string(), "dev".to_string()],
        env: vec![],
        cwd: Some("app".to_string()),
    };

    let scenario =
        build_debug_scenario_from_task(&task, "Debug App".to_string(), "Dart".to_string())
            .expect("scenario");
    let parsed: serde_json::Value =
        serde_json::from_str(&scenario.config).expect("valid generated json");

    assert_eq!(scenario.label, "Debug App");
    assert_eq!(parsed["type"], "flutter");
    assert_eq!(parsed["program"], "lib/main.dart");
    assert_eq!(parsed["cwd"], "app");
    assert_eq!(
        parsed["toolArgs"],
        json!(["--flavor", "dev", "-d", "chrome"])
    );
}

#[test]
fn locator_creates_dart_scenario_from_dart_run_task() {
    let task = TaskTemplate {
        label: "Run CLI".to_string(),
        command: "dart".to_string(),
        args: vec![
            "run".to_string(),
            "bin/main.dart".to_string(),
            "--observe".to_string(),
        ],
        env: vec![("FOO".to_string(), "bar".to_string())],
        cwd: Some("cli".to_string()),
    };

    let scenario =
        build_debug_scenario_from_task(&task, "Debug CLI".to_string(), "Dart".to_string())
            .expect("scenario");
    let parsed: serde_json::Value =
        serde_json::from_str(&scenario.config).expect("valid generated json");

    assert_eq!(scenario.label, "Debug CLI");
    assert_eq!(parsed["type"], "dart");
    assert_eq!(parsed["program"], "bin/main.dart");
    assert_eq!(parsed["cwd"], "cli");
    assert_eq!(parsed["toolArgs"], json!(["--observe"]));
    assert_eq!(parsed["env"], json!({ "FOO": "bar" }));
}

#[test]
fn locator_creates_flutter_scenario_from_fvm_flutter_run_task() {
    let task = TaskTemplate {
        label: "Run App (FVM)".to_string(),
        command: "fvm".to_string(),
        args: vec![
            "flutter".to_string(),
            "run".to_string(),
            "-d".to_string(),
            "macos".to_string(),
        ],
        env: vec![],
        cwd: Some("app".to_string()),
    };

    let scenario =
        build_debug_scenario_from_task(&task, "Debug App".to_string(), "Dart".to_string())
            .expect("scenario");
    let parsed: serde_json::Value =
        serde_json::from_str(&scenario.config).expect("valid generated json");

    assert_eq!(scenario.label, "Debug App");
    assert_eq!(parsed["type"], "flutter");
    assert_eq!(parsed["useFvm"], true);
    assert_eq!(parsed["toolArgs"], json!(["-d", "macos"]));
}

#[test]
fn locator_creates_dart_scenario_from_fvm_dart_run_task() {
    let task = TaskTemplate {
        label: "Run CLI (FVM)".to_string(),
        command: "fvm".to_string(),
        args: vec![
            "dart".to_string(),
            "run".to_string(),
            "bin/main.dart".to_string(),
            "--observe".to_string(),
        ],
        env: vec![],
        cwd: Some("cli".to_string()),
    };

    let scenario =
        build_debug_scenario_from_task(&task, "Debug CLI".to_string(), "Dart".to_string())
            .expect("scenario");
    let parsed: serde_json::Value =
        serde_json::from_str(&scenario.config).expect("valid generated json");

    assert_eq!(scenario.label, "Debug CLI");
    assert_eq!(parsed["type"], "dart");
    assert_eq!(parsed["useFvm"], true);
    assert_eq!(parsed["toolArgs"], json!(["--observe"]));
}

#[test]
fn locator_creates_flutter_profile_scenario() {
    let task = TaskTemplate {
        label: "Profile App".to_string(),
        command: "flutter".to_string(),
        args: vec!["run".to_string(), "--profile".to_string()],
        env: vec![],
        cwd: Some("app".to_string()),
    };

    let scenario =
        build_debug_scenario_from_task(&task, "Profile App".to_string(), "Dart".to_string())
            .expect("scenario");
    let parsed: serde_json::Value =
        serde_json::from_str(&scenario.config).expect("valid generated json");

    assert_eq!(scenario.label, "Profile App");
    assert_eq!(parsed["type"], "flutter");
    assert_eq!(parsed["flutterMode"], "profile");
    assert_eq!(parsed["toolArgs"], json!(["-d", "chrome"]));
}

#[test]
fn locator_rejects_unrelated_tasks() {
    let task = TaskTemplate {
        label: "flutter test".to_string(),
        command: "flutter".to_string(),
        args: vec!["test".to_string()],
        env: vec![],
        cwd: None,
    };

    assert!(
        build_debug_scenario_from_task(&task, "Test".to_string(), "Dart".to_string()).is_none()
    );
}

#[test]
fn parse_flutter_devices_json_parses_machine_output() {
    let devices = parse_flutter_devices_json(
        r#"[
            {
                "id":"chrome",
                "name":"Chrome",
                "platformType":"web",
                "emulator":false,
                "category":"web"
            },
            {
                "id":"macos",
                "name":"macOS",
                "platformType":"desktop",
                "emulator":false,
                "category":"desktop"
            }
        ]"#,
    )
    .expect("devices");

    assert_eq!(
        devices,
        vec![
            FlutterDevice {
                id: "chrome".to_string(),
                name: "Chrome".to_string(),
                platform: Some("web".to_string()),
                emulator: Some(false),
                category: Some("web".to_string())
            },
            FlutterDevice {
                id: "macos".to_string(),
                name: "macOS".to_string(),
                platform: Some("desktop".to_string()),
                emulator: Some(false),
                category: Some("desktop".to_string())
            }
        ]
    );
}

#[test]
fn filter_flutter_devices_matches_id_and_name() {
    let devices = vec![
        FlutterDevice {
            id: "chrome".to_string(),
            name: "Chrome".to_string(),
            platform: Some("web".to_string()),
            emulator: Some(false),
            category: Some("web".to_string()),
        },
        FlutterDevice {
            id: "macos".to_string(),
            name: "macOS".to_string(),
            platform: Some("desktop".to_string()),
            emulator: Some(false),
            category: Some("desktop".to_string()),
        },
    ];

    assert_eq!(filter_flutter_devices(devices.clone(), "chrome").len(), 1);
    assert_eq!(filter_flutter_devices(devices.clone(), "desktop").len(), 1);
    assert_eq!(filter_flutter_devices(devices, "").len(), 2);
}

#[test]
fn format_flutter_devices_output_lists_devices() {
    let output = format_flutter_devices_output(&[FlutterDevice {
        id: "chrome".to_string(),
        name: "Chrome".to_string(),
        platform: Some("web".to_string()),
        emulator: Some(false),
        category: Some("web".to_string()),
    }]);

    assert!(output.text.contains("Chrome"));
    assert!(output.text.contains("`chrome`"));
    assert_eq!(output.sections.len(), 1);
    assert_eq!(output.sections[0].label, "Flutter Devices");
}

#[test]
fn build_devtools_url_accepts_http_and_ws_uris() {
    assert_eq!(
        build_devtools_url("http://127.0.0.1:50300/").expect("url"),
        "http://127.0.0.1:9100?uri=http%3A%2F%2F127.0.0.1%3A50300%2F"
    );
    assert_eq!(
        build_devtools_url("ws://127.0.0.1:50300/ws").expect("url"),
        "http://127.0.0.1:9100?uri=ws%3A%2F%2F127.0.0.1%3A50300%2Fws"
    );
}

#[test]
fn build_devtools_url_normalizes_bare_host_port_values() {
    assert_eq!(
        build_devtools_url("127.0.0.1:50300").expect("url"),
        "http://127.0.0.1:9100?uri=http%3A%2F%2F127.0.0.1%3A50300"
    );
}

#[test]
fn build_devtools_url_rejects_empty_values() {
    assert!(build_devtools_url("   ").is_err());
}

#[test]
fn normalize_vm_service_uri_accepts_bare_host_port_values() {
    assert_eq!(
        normalize_vm_service_uri("127.0.0.1:50300").expect("uri"),
        "http://127.0.0.1:50300"
    );
    assert_eq!(
        normalize_vm_service_uri("ws://127.0.0.1:50300/ws").expect("uri"),
        "ws://127.0.0.1:50300/ws"
    );
}

#[test]
fn normalize_vm_service_uri_rejects_empty_values() {
    assert!(normalize_vm_service_uri(" ").is_err());
}

#[test]
fn vm_service_websocket_uri_normalizes_http_and_ws_inputs() {
    assert_eq!(
        vm_service_websocket_uri("127.0.0.1:50300").expect("uri"),
        "ws://127.0.0.1:50300/ws"
    );
    assert_eq!(
        vm_service_websocket_uri("http://127.0.0.1:50300/").expect("uri"),
        "ws://127.0.0.1:50300/ws"
    );
    assert_eq!(
        vm_service_websocket_uri("ws://127.0.0.1:50300/ws").expect("uri"),
        "ws://127.0.0.1:50300/ws"
    );
}

#[test]
fn build_flutter_attach_shell_script_includes_requested_action() {
    let script = build_flutter_attach_shell_script(
        "flutter",
        "C:\\src\\app's folder",
        "ws://127.0.0.1:50300/ws",
        'R',
        "hot restart",
    );

    assert!(script.contains("attach --debug-uri "));
    assert!(script.contains(" --project-root "));
    assert!(script.contains("--report-ready"));
    assert!(script.contains("$p.StandardInput.WriteLine('R');"));
    assert!(script.contains("hot restart"));
    assert!(script.contains("app''s folder"));
}

#[test]
fn preferred_windows_tool_path_uses_dart_bat_when_present_next_to_resolved_dart() {
    let root = std::env::temp_dir().join(format!(
        "zed-dart-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    let bin = root.join("flutter").join("bin");
    fs::create_dir_all(&bin).expect("bin");
    let dart_exe = bin.join("dart.exe");
    let dart_bat = bin.join("dart.bat");
    fs::write(&dart_exe, "").expect("dart exe");
    fs::write(&dart_bat, "").expect("dart bat");

    let preferred =
        preferred_windows_tool_path("dart.exe", Some(dart_exe.to_string_lossy().into()));
    assert_eq!(preferred, dart_bat.to_string_lossy());

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_vm_service_info_file_path_generates_unique_flutter_launch_files() {
    let root = std::env::temp_dir().join(format!(
        "zed-dart-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&root).expect("root");

    let first =
        resolve_vm_service_info_file_path(&root.to_string_lossy(), "flutter", "launch", None)
            .expect("first path")
            .expect("first value");
    let second =
        resolve_vm_service_info_file_path(&root.to_string_lossy(), "flutter", "launch", None)
            .expect("second path")
            .expect("second value");

    assert_ne!(first, second);
    assert!(first.contains("flutter-vmservice-"));
    assert!(Path::new(&first)
        .parent()
        .expect("parent")
        .ends_with("vmservice-info"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn read_vm_service_uri_from_info_file_supports_common_keys() {
    let root = std::env::temp_dir().join(format!(
        "zed-dart-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&root).expect("root");
    let file = root.join("vmservice.json");
    fs::write(&file, r#"{ "uri": "ws://127.0.0.1:50300/ws" }"#).expect("write");

    let uri = read_vm_service_uri_from_info_file(&file).expect("uri");
    assert_eq!(uri, "ws://127.0.0.1:50300/ws");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn read_vm_service_uri_from_info_file_rejects_invalid_json() {
    let root = std::env::temp_dir().join(format!(
        "zed-dart-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&root).expect("root");
    let file = root.join("vmservice.json");
    fs::write(&file, "{ not json").expect("write");

    assert!(read_vm_service_uri_from_info_file(&file).is_err());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_flutter_hot_command_target_uses_latest_info_file_when_input_is_empty() {
    let root = std::env::temp_dir().join(format!(
        "zed-dart-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    let info_dir = root.join(".zed").join("dart").join("vmservice-info");
    fs::create_dir_all(&info_dir).expect("info dir");

    let older = info_dir.join("older.json");
    let newer = info_dir.join("newer.json");
    fs::write(&older, r#"{ "uri": "ws://127.0.0.1:50300/ws" }"#).expect("older");
    std::thread::sleep(std::time::Duration::from_millis(5));
    fs::write(&newer, r#"{ "uri": "ws://127.0.0.1:50301/ws" }"#).expect("newer");

    let (uri, source) =
        resolve_flutter_hot_command_target_for_root(&root.to_string_lossy(), "").expect("target");
    assert_eq!(uri, "ws://127.0.0.1:50301/ws");
    assert!(source.contains("newer.json"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_flutter_hot_command_target_supports_relative_info_file_paths() {
    let root = std::env::temp_dir().join(format!(
        "zed-dart-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(root.join(".dart_tool")).expect("root");
    fs::write(
        root.join(".dart_tool").join("vm.json"),
        r#"{ "uri": "127.0.0.1:50300" }"#,
    )
    .expect("write");

    let (uri, source) =
        resolve_flutter_hot_command_target_for_root(&root.to_string_lossy(), ".dart_tool/vm.json")
            .expect("target");
    assert_eq!(uri, "http://127.0.0.1:50300");
    assert!(source.contains(".dart_tool"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_flutter_hot_command_target_supports_direct_uris() {
    let (uri, source) = resolve_flutter_hot_command_target_for_root("workspace", "127.0.0.1:50300")
        .expect("target");
    assert_eq!(uri, "http://127.0.0.1:50300");
    assert_eq!(source, "Using provided VM service URI");
}

#[test]
fn build_debug_adapter_binary_supports_fvm_for_dart() {
    let config = json!({
        "type": "dart",
        "request": "launch",
        "program": "bin/main.dart",
        "useFvm": true
    });

    let binary =
        build_debug_adapter_binary_from_config(&config, "workspace").expect("adapter binary");
    assert_eq!(binary.command.as_deref(), Some("fvm"));
    assert_eq!(binary.arguments, vec!["dart", "debug_adapter"]);
}

#[test]
fn build_debug_adapter_binary_supports_fvm_for_flutter() {
    let config = json!({
        "type": "flutter",
        "request": "launch",
        "program": "lib/main.dart",
        "useFvm": true
    });

    let binary =
        build_debug_adapter_binary_from_config(&config, "workspace").expect("adapter binary");
    assert_eq!(binary.command.as_deref(), Some("fvm"));
    assert_eq!(binary.arguments, vec!["flutter", "debug_adapter"]);
}
