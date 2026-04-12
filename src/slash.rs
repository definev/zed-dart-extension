use std::path::Path;

use tungstenite::{connect, Message};
use zed_extension_api::process::Command as ProcessCommand;
use zed_extension_api::{
    serde_json, serde_json::json, Result, SlashCommandOutput, SlashCommandOutputSection, Worktree,
};

use crate::flutter::{
    discover_latest_vm_service_info_file, normalize_vm_service_uri,
    read_vm_service_uri_from_info_file, vm_service_websocket_uri,
};

fn percent_encode_uri_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

pub(crate) fn build_devtools_url(vm_service_uri: &str) -> Result<String, String> {
    let trimmed = vm_service_uri.trim();
    if trimmed.is_empty() {
        return Err("VM service URI is required".to_string());
    }

    let normalized = if trimmed.starts_with("ws://") || trimmed.starts_with("wss://") {
        trimmed.to_string()
    } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    };

    let uri: serde_json::Value = json!(normalized);
    let normalized = uri
        .as_str()
        .ok_or_else(|| "Failed to normalize VM service URI".to_string())?;

    Ok(format!(
        "http://127.0.0.1:9100?uri={}",
        percent_encode_uri_component(normalized)
    ))
}

pub(crate) fn probe_vm_service(vm_service_uri: &str) -> Result<SlashCommandOutput, String> {
    let websocket_uri = vm_service_websocket_uri(vm_service_uri)?;
    let (mut socket, _) = connect(&websocket_uri)
        .map_err(|err| format!("Failed to connect to VM service `{websocket_uri}`: {err}"))?;

    let request = json!({
        "jsonrpc": "2.0",
        "id": "zed-dart-probe",
        "method": "getVM",
    })
    .to_string();

    socket
        .send(Message::Text(request.into()))
        .map_err(|err| format!("Failed to send VM service request to `{websocket_uri}`: {err}"))?;

    let message = socket.read().map_err(|err| {
        format!("Failed to read VM service response from `{websocket_uri}`: {err}")
    })?;

    let payload = match message {
        Message::Text(text) => text.to_string(),
        Message::Binary(bytes) => String::from_utf8(bytes.to_vec())
            .map_err(|err| format!("VM service response was not valid UTF-8: {err}"))?,
        other => {
            return Err(format!(
                "Unexpected VM service response from `{websocket_uri}`: {other:?}"
            ))
        }
    };

    let parsed: serde_json::Value = serde_json::from_str(&payload)
        .map_err(|err| format!("Invalid VM service JSON response from `{websocket_uri}`: {err}"))?;

    let text = serde_json::to_string_pretty(&parsed)
        .map_err(|err| format!("Failed to format VM service response: {err}"))?;

    Ok(SlashCommandOutput {
        sections: vec![SlashCommandOutputSection {
            range: (0..text.len()).into(),
            label: "Flutter VM Probe".to_string(),
        }],
        text,
    })
}

pub(crate) fn resolve_flutter_hot_command_target_for_root(
    worktree_root: &str,
    input: &str,
) -> Result<(String, String), String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        let path = discover_latest_vm_service_info_file(worktree_root)?;
        let uri = read_vm_service_uri_from_info_file(&path)?;
        return Ok((
            uri,
            format!("Using VM service info file `{}`", path.display()),
        ));
    }

    let candidate_path = Path::new(trimmed);
    if candidate_path.extension().and_then(|ext| ext.to_str()) == Some("json")
        || trimmed.contains('\\')
        || trimmed.contains('/')
        || trimmed.starts_with('.')
    {
        let path = if candidate_path.is_absolute() {
            candidate_path.to_path_buf()
        } else {
            Path::new(worktree_root).join(candidate_path)
        };
        let uri = read_vm_service_uri_from_info_file(&path)?;
        return Ok((
            uri,
            format!("Using VM service info file `{}`", path.display()),
        ));
    }

    Ok((
        normalize_vm_service_uri(trimmed)?,
        "Using provided VM service URI".to_string(),
    ))
}

pub(crate) fn resolve_flutter_hot_command_target(
    worktree: &Worktree,
    input: &str,
) -> Result<(String, String), String> {
    resolve_flutter_hot_command_target_for_root(&worktree.root_path(), input)
}

pub(crate) fn quote_powershell_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub(crate) fn build_flutter_attach_shell_script(
    flutter: &str,
    project_root: &str,
    vm_service_uri: &str,
    action_key: char,
    action_label: &str,
) -> String {
    let flutter = quote_powershell_literal(flutter);
    let project_root = quote_powershell_literal(project_root);
    let vm_service_uri = quote_powershell_literal(vm_service_uri);

    format!(
        concat!(
            "$psi = New-Object System.Diagnostics.ProcessStartInfo;",
            "$psi.FileName = {flutter};",
            "$psi.Arguments = 'attach --debug-uri ' + {vm_service_uri} + ' --project-root ' + {project_root} + ' --report-ready';",
            "$psi.WorkingDirectory = {project_root};",
            "$psi.UseShellExecute = $false;",
            "$psi.RedirectStandardInput = $true;",
            "$psi.RedirectStandardOutput = $true;",
            "$psi.RedirectStandardError = $true;",
            "$p = New-Object System.Diagnostics.Process;",
            "$p.StartInfo = $psi;",
            "$stdout = New-Object System.Text.StringBuilder;",
            "$stderr = New-Object System.Text.StringBuilder;",
            "$ready = New-Object System.Threading.ManualResetEventSlim($false);",
            "$stderrReady = New-Object System.Threading.ManualResetEventSlim($false);",
            "$handler = [System.Diagnostics.DataReceivedEventHandler]{{",
            " param($sender, $eventArgs)",
            " if ($null -ne $eventArgs.Data) {{",
            "  [void]$stdout.AppendLine($eventArgs.Data);",
            "  if ($eventArgs.Data -match 'Flutter run key commands|The Flutter DevTools debugger and profiler|ready|A Dart VM Service') {{ $ready.Set() }}",
            " }}",
            "}};",
            "$errorHandler = [System.Diagnostics.DataReceivedEventHandler]{{",
            " param($sender, $eventArgs)",
            " if ($null -ne $eventArgs.Data) {{",
            "  [void]$stderr.AppendLine($eventArgs.Data);",
            "  if ($eventArgs.Data -match 'Flutter run key commands|The Flutter DevTools debugger and profiler|ready|A Dart VM Service') {{ $stderrReady.Set() }}",
            " }}",
            "}};",
            "$p.add_OutputDataReceived($handler);",
            "$p.add_ErrorDataReceived($errorHandler);",
            "if (-not $p.Start()) {{ throw 'Failed to start flutter attach.' }};",
            "$p.BeginOutputReadLine();",
            "$p.BeginErrorReadLine();",
            "if (-not ($ready.Wait(20000) -or $stderrReady.Wait(20000))) {{",
            " try {{ if (-not $p.HasExited) {{ $p.Kill() }} }} catch {{}};",
            " throw 'Timed out waiting for flutter attach to become ready.'",
            "}};",
            "$p.StandardInput.WriteLine('{action_key}');",
            "$p.StandardInput.WriteLine('q');",
            "$p.StandardInput.Flush();",
            "if (-not $p.WaitForExit(30000)) {{",
            " try {{ $p.Kill() }} catch {{}};",
            " throw 'Timed out waiting for flutter attach to exit after {action_label}.'",
            "}};",
            "$combined = (($stdout.ToString() + \"`n\" + $stderr.ToString()).Trim());",
            "if ($p.ExitCode -ne 0 -and $combined -notmatch 'ready|Reloaded|Restarted|performing hot reload|performing hot restart') {{",
            " throw ('flutter attach exited with code ' + $p.ExitCode + ': ' + $combined)",
            "}};",
            "Write-Output ('Flutter {action_label} requested via shell attach shim.' + [Environment]::NewLine + $combined.Trim())"
        ),
        flutter = flutter,
        project_root = project_root,
        vm_service_uri = vm_service_uri,
        action_key = action_key,
        action_label = action_label
    )
}

pub(crate) fn run_flutter_attach_action(
    worktree: &Worktree,
    vm_service_uri: &str,
    action_key: char,
    action_label: &str,
) -> Result<SlashCommandOutput, String> {
    let flutter = worktree
        .which("flutter")
        .unwrap_or_else(|| "flutter".to_string());
    let (vm_service_uri, source) = resolve_flutter_hot_command_target(worktree, vm_service_uri)?;
    let script = build_flutter_attach_shell_script(
        &flutter,
        &worktree.root_path(),
        &vm_service_uri,
        action_key,
        action_label,
    );

    let output = ProcessCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg(script)
        .envs(worktree.shell_env())
        .output()?;

    if output.status != Some(0) {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let message = if !stderr.is_empty() { stderr } else { stdout };
        return Err(format!(
            "Failed to trigger Flutter {action_label}{}",
            if message.is_empty() {
                String::new()
            } else {
                format!(": {message}")
            }
        ));
    }

    let command_output = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let text = if command_output.is_empty() {
        source
    } else {
        format!("{source}\n{command_output}")
    };
    Ok(SlashCommandOutput {
        sections: vec![SlashCommandOutputSection {
            range: (0..text.len()).into(),
            label: format!("Flutter {action_label}"),
        }],
        text,
    })
}
